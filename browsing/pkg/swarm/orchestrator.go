package swarm

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/reclamation-admin/agentic-browser-go/pkg/sitemap"
	"github.com/reclamation-admin/agentic-browser-go/pkg/swarm/proto"
	"github.com/reclamation-admin/agentic-browser-go/pkg/vault"
	"google.golang.org/grpc"
)

// MissionState tracks the lifecycle of a mission for the mobile interface.
type MissionState struct {
	MissionID    string
	Status       string // "pending", "running", "success", "failed"
	ResultJSON   string
	DispatchedTo string
}

type registeredNode struct {
	status    *proto.NodeStatus
	maxSimops int32
}

// Orchestrator coordinates missions across remote nodes.
type Orchestrator struct {
	proto.UnimplementedOrchestratorServiceServer
	ledgerPath      string
	ledgerMu        sync.Mutex
	sm              *sitemap.SiteMap
	vault           *vault.Vault
	missionsMu      sync.RWMutex
	missionStatuses map[string]*MissionState
	nodesMu         sync.Mutex
	nodes           map[string]*registeredNode
}

// NewOrchestrator creates a new orchestrator with a ledger file path.
func NewOrchestrator(ledgerPath string, sm *sitemap.SiteMap) *Orchestrator {
	os.MkdirAll("missions", 0755)
	os.MkdirAll("vault", 0755)
	
	v := vault.NewVault("vault/secrets.enc")
	masterPwd := os.Getenv("VAULT_PASSWORD")
	if masterPwd != "" {
		if err := v.Load(masterPwd); err != nil {
			fmt.Printf("[Orchestrator] Warning: Could not load vault: %v\n", err)
		} else {
			fmt.Println("[Orchestrator] Vault loaded successfully.")
		}
	}
	
	return &Orchestrator{
		ledgerPath:      ledgerPath,
		sm:              sm,
		vault:           v,
		missionStatuses: make(map[string]*MissionState),
		nodes:           make(map[string]*registeredNode),
	}
}

// DispatchMission sends a task to a remote node and logs the result.
func (o *Orchestrator) DispatchMission(ctx context.Context, nodeAddr string, req *proto.SpawnAgentRequest) (*proto.SpawnAgentResponse, error) {
	conn, err := grpc.Dial(nodeAddr, grpc.WithInsecure())
	if err != nil {
		return nil, fmt.Errorf("failed to connect to node %s: %v", nodeAddr, err)
	}
	defer conn.Close()

	client := proto.NewSwarmServiceClient(conn)
	
	if err := o.createMissionFile(req); err != nil {
		return nil, fmt.Errorf("failed to create mission file: %v", err)
	}

	fmt.Printf("[Orchestrator] Dispatching mission %s to %s...\n", req.MissionId, nodeAddr)
	resp, err := client.SpawnAgent(ctx, req)
	if err != nil {
		return nil, fmt.Errorf("failed to spawn agent: %v", err)
	}

	if resp.Success && resp.ResultJson != "" {
		fmt.Printf("[Orchestrator] Mission %s completed. Appending to ledger.\n", req.MissionId)
		if err := o.AppendToLedger(req.MissionId, resp.ResultJson); err != nil {
			fmt.Printf("[Orchestrator] Warning: Failed to append to ledger: %v\n", err)
		}
		
		fmt.Printf("[Orchestrator] Recording flow for %s...\n", req.Url)
		if err := o.RecordFlow(ctx, req.Url, resp.ResultJson); err != nil {
			fmt.Printf("[Orchestrator] Warning: Failed to record flow: %v\n", err)
		}
	}

	return resp, nil
}

func (o *Orchestrator) createMissionFile(req *proto.SpawnAgentRequest) error {
	filename := fmt.Sprintf("missions/mission_%s.json", req.MissionId)
	data, err := json.MarshalIndent(req, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filename, data, 0644)
}

func (o *Orchestrator) AppendToLedger(missionId string, resultJson string) error {
	o.ledgerMu.Lock()
	defer o.ledgerMu.Unlock()

	f, err := os.OpenFile(o.ledgerPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return err
	}
	defer f.Close()

	var resultObj map[string]interface{}
	json.Unmarshal([]byte(resultJson), &resultObj)

	entry := map[string]interface{}{
		"mission_id": missionId,
		"timestamp":  time.Now().Format(time.RFC3339),
		"status":     resultObj["status"],
		"reason":     resultObj["reason"],
		"url":        resultObj["url"],
		"result":     json.RawMessage(resultJson),
	}
	
	data, err := json.Marshal(entry)
	if err != nil {
		return err
	}
	
	_, err = f.Write(append(data, '\n'))
	return err
}

// RecordFlow saves the page structure and fields as SiteMap triples.
func (o *Orchestrator) RecordFlow(ctx context.Context, url string, resultJson string) error {
	var result struct {
		Fields  map[string]string `json:"fields"`
		AntiBot string            `json:"anti_bot"`
	}
	if err := json.Unmarshal([]byte(resultJson), &result); err != nil {
		return fmt.Errorf("failed to parse result JSON: %v", err)
	}

	urlHash := o.sm.RegisterString(url)
	antiBotHash := o.sm.RegisterString(result.AntiBot)

	o.saveTriple(urlHash, sitemap.PredicateURL, urlHash)
	o.saveTriple(urlHash, sitemap.PredicateStyle, antiBotHash)

	for fieldName, selector := range result.Fields {
		fieldID := fmt.Sprintf("%s_field_%s", url, fieldName)
		fieldHash := o.sm.RegisterString(fieldID)
		nameHash := o.sm.RegisterString(fieldName)
		selHash := o.sm.RegisterString(selector)

		o.saveTriple(urlHash, sitemap.PredicateHasChild, fieldHash)
		o.saveTriple(fieldHash, sitemap.PredicateName, nameHash)
		o.saveTriple(fieldHash, sitemap.PredicateValue, selHash)
	}

	return nil
}

// RecordTransition saves a navigation link between two pages.
func (o *Orchestrator) RecordTransition(ctx context.Context, fromUrl, toUrl, action string) error {
	fromHash := o.sm.RegisterString(fromUrl)
	toHash := o.sm.RegisterString(toUrl)
	actionHash := o.sm.RegisterString(action)

	o.saveTriple(fromHash, sitemap.PredicateLinksTo, toHash)
	o.saveTriple(toHash, sitemap.PredicateName, actionHash)
	return nil
}

// FindShortestPath queries the SiteMap graph for the shortest path between two URLs.
func (o *Orchestrator) FindShortestPath(ctx context.Context, startUrl, targetUrl string) ([]string, error) {
	startHash := sitemap.HashString(startUrl)
	targetHash := sitemap.HashString(targetUrl)

	triples, err := LoadAllTriples("sitemap_db")
	if err != nil {
		return nil, err
	}

	adj := make(map[uint64][]uint64)
	for _, t := range triples {
		if t.PredicateID == sitemap.PredicateLinksTo {
			adj[t.SubjectHash] = append(adj[t.SubjectHash], t.ObjectHash)
		}
	}

	queue := [][]uint64{{startHash}}
	visited := make(map[uint64]bool)
	visited[startHash] = true

	var path []uint64
	for len(queue) > 0 {
		curr := queue[0]
		queue = queue[1:]

		last := curr[len(curr)-1]
		if last == targetHash {
			path = curr
			break
		}

		for _, next := range adj[last] {
			if !visited[next] {
				visited[next] = true
				nextPath := make([]uint64, len(curr)+1)
				copy(nextPath, curr)
				nextPath[len(curr)] = next
				queue = append(queue, nextPath)
			}
		}
	}

	if len(path) == 0 {
		return nil, fmt.Errorf("no path found")
	}

	var urls []string
	for _, h := range path {
		if val, ok := o.sm.ResolveString(h); ok {
			urls = append(urls, val)
		}
	}
	return urls, nil
}

// FindPagesByFields finds pages that contain ALL of the specified field names.
func (o *Orchestrator) FindPagesByFields(ctx context.Context, fields []string) ([]string, error) {
	triples, err := LoadAllTriples("sitemap_db")
	if err != nil {
		return nil, err
	}

	fieldNames := make(map[uint64]string)
	for _, t := range triples {
		if t.PredicateID == sitemap.PredicateName {
			if val, ok := o.sm.ResolveString(t.ObjectHash); ok {
				fieldNames[t.SubjectHash] = val
			}
		}
	}

	pageFields := make(map[uint64]map[string]bool)
	for _, t := range triples {
		if t.PredicateID == sitemap.PredicateHasChild {
			if fName, ok := fieldNames[t.ObjectHash]; ok {
				if pageFields[t.SubjectHash] == nil {
					pageFields[t.SubjectHash] = make(map[string]bool)
				}
				pageFields[t.SubjectHash][fName] = true
			}
		}
	}

	var matchingUrls []string
	for pageHash, hasFields := range pageFields {
		allMatch := true
		for _, reqField := range fields {
			if !hasFields[reqField] {
				allMatch = false
				break
			}
		}
		if allMatch {
			if urlVal, ok := o.sm.ResolveString(pageHash); ok {
				matchingUrls = append(matchingUrls, urlVal)
			}
		}
	}

	return matchingUrls, nil
}

func (o *Orchestrator) RecordSiteCategory(ctx context.Context, domain string, category string) error {
	domainHash := o.sm.RegisterString(domain)
	catHash := o.sm.RegisterString(category)
	o.saveTriple(domainHash, sitemap.PredicateStyle, catHash)
	return nil
}

func (o *Orchestrator) FindSitesByCategory(ctx context.Context, category string) ([]string, error) {
	triples, err := LoadAllTriples("sitemap_db")
	if err != nil {
		return nil, err
	}

	var domains []string
	for _, t := range triples {
		if t.PredicateID == sitemap.PredicateStyle {
			if val, ok := o.sm.ResolveString(t.ObjectHash); ok && val == category {
				if domVal, ok := o.sm.ResolveString(t.SubjectHash); ok {
					domains = append(domains, domVal)
				}
			}
		}
	}
	return domains, nil
}

func (o *Orchestrator) RouteMission(ctx context.Context, prompt string) (string, error) {
	category, err := o.ExtractIntent(ctx, prompt)
	if err != nil {
		fmt.Printf("[Orchestrator] Warning: Failed to extract intent via LM Studio: %v. Falling back to rules.\n", err)
		
		category = ""
		promptLower := strings.ToLower(prompt)
		
		if strings.Contains(promptLower, "laptop") || strings.Contains(promptLower, "phone") || strings.Contains(promptLower, "electronics") {
			category = "Electronics"
		} else if strings.Contains(promptLower, "pants") || strings.Contains(promptLower, "shoes") || strings.Contains(promptLower, "fashion") {
			category = "Fashion"
		} else if strings.Contains(promptLower, "buy") || strings.Contains(promptLower, "shop") || strings.Contains(promptLower, "retail") {
			category = "General Retail"
		}
	}
	
	if category == "" {
		return "", fmt.Errorf("could not determine category for prompt: %s", prompt)
	}
	
	fmt.Printf("[Orchestrator] Determined category: %s\n", category)
	
	domains, err := o.FindSitesByCategory(ctx, category)
	if err != nil {
		return "", err
	}
	
	if len(domains) == 0 {
		return "", fmt.Errorf("no sites found in category: %s", category)
	}
	
	return domains[0], nil
}

func (o *Orchestrator) ExtractIntent(ctx context.Context, prompt string) (string, error) {
	urls := []string{
		os.Getenv("LM_STUDIO_URL"),
		os.Getenv("LM_STUDIO_URL_FALLBACK"),
	}

	var lastErr error
	for _, url := range urls {
		if url == "" {
			continue
		}

		fmt.Printf("[Orchestrator] Trying LM Studio at %s...\n", url)
		
		payload := map[string]interface{}{
			"model": "local-model",
			"messages": []map[string]string{
				{
					"role":    "system",
					"content": "You are an intent extractor. Categorize the user's prompt into one of these categories: Electronics, Fashion, General Retail, Travel. Return ONLY the category name.",
				},
				{
					"role":    "user",
					"content": prompt,
				},
			},
			"temperature": 0.1,
		}

		jsonBytes, _ := json.Marshal(payload)
		
		req, err := http.NewRequestWithContext(ctx, "POST", url+"/chat/completions", bytes.NewBuffer(jsonBytes))
		if err != nil {
			lastErr = err
			continue
		}
		req.Header.Set("Content-Type", "application/json")

		client := &http.Client{Timeout: 5 * time.Second}
		resp, err := client.Do(req)
		if err != nil {
			lastErr = err
			continue
		}
		defer resp.Body.Close()

		if resp.StatusCode != http.StatusOK {
			lastErr = fmt.Errorf("status: %d", resp.StatusCode)
			continue
		}

		var result map[string]interface{}
		if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
			lastErr = err
			continue
		}

		choices, ok := result["choices"].([]interface{})
		if !ok || len(choices) == 0 {
			lastErr = fmt.Errorf("invalid response")
			continue
		}
		firstChoice := choices[0].(map[string]interface{})
		message := firstChoice["message"].(map[string]interface{})
		content := message["content"].(string)

		return strings.TrimSpace(content), nil
	}

	if lastErr != nil {
		return "", fmt.Errorf("all LM Studio endpoints failed. Last error: %v", lastErr)
	}
	return "", fmt.Errorf("no LM Studio endpoints configured")
}

func (o *Orchestrator) SelectBestNode(ctx context.Context, missionReq *proto.SpawnAgentRequest) (string, error) {
	o.nodesMu.Lock()
	defer o.nodesMu.Unlock()

	intensity := o.EstimateIntensity(ctx, missionReq.SystemInstruction)
	fmt.Printf("[Orchestrator] Selecting node for mission %s (Intensity: %s)\n", missionReq.MissionId, intensity)

	var bestNode string
	var maxAvailable int32 = -1
	nowMs := time.Now().UnixNano() / int64(time.Millisecond)

	for _, n := range o.nodes {
		if n.status.LastSeenMs > nowMs-30000 && n.status.CpuUtilization < 90.0 {
			available := n.maxSimops - n.status.CurrentSimops
			if available > 0 && available > maxAvailable {
				maxAvailable = available
				bestNode = n.status.Endpoint
			}
		}
	}
	if bestNode == "" {
		for _, n := range o.nodes {
			bestNode = n.status.Endpoint
			break
		}
	}
	if bestNode == "" {
		return "", fmt.Errorf("no suitable nodes available")
	}
	return bestNode, nil
}

func (o *Orchestrator) EstimateIntensity(ctx context.Context, instruction string) string {
	intensity, err := o.ClassifyIntensity(ctx, instruction)
	if err == nil && (intensity == "low" || intensity == "medium" || intensity == "high") {
		return intensity
	}

	instr := strings.ToLower(instruction)
	highKeywords := []string{"batch", "scrape", "all", "video", "extract everything", "crawl", "full site", "thousands", "intensive"}
	for _, kw := range highKeywords {
		if strings.Contains(instr, kw) {
			return "high"
		}
	}

	medKeywords := []string{"search", "find", "checkout", "buy", "form", "login", "register", "complex", "book"}
	for _, kw := range medKeywords {
		if strings.Contains(instr, kw) {
			return "medium"
		}
	}

	return "low"
}

func (o *Orchestrator) ClassifyIntensity(ctx context.Context, instruction string) (string, error) {
	urls := []string{
		os.Getenv("LM_STUDIO_URL"),
		os.Getenv("LM_STUDIO_URL_FALLBACK"),
	}

	for _, url := range urls {
		if url == "" { continue }
		
		payload := map[string]interface{}{
			"model": "local-model",
			"messages": []map[string]string{
				{
					"role":    "system",
					"content": "You are a mission architect. Categorize the computational intensity of the following browser mission into exactly one of: low, medium, high. Return ONLY the lowercase word.",
				},
				{
					"role":    "user",
					"content": instruction,
				},
			},
			"temperature": 0.0,
		}

		jsonBytes, _ := json.Marshal(payload)
		req, err := http.NewRequestWithContext(ctx, "POST", url+"/chat/completions", bytes.NewBuffer(jsonBytes))
		if err != nil { continue }
		req.Header.Set("Content-Type", "application/json")

		client := &http.Client{Timeout: 3 * time.Second}
		resp, err := client.Do(req)
		if err != nil { continue }
		defer resp.Body.Close()

		if resp.StatusCode == http.StatusOK {
			var result map[string]interface{}
			json.NewDecoder(resp.Body).Decode(&result)
			if choices, ok := result["choices"].([]interface{}); ok && len(choices) > 0 {
				content := choices[0].(map[string]interface{})["message"].(map[string]interface{})["content"].(string)
				return strings.TrimSpace(strings.ToLower(content)), nil
			}
		}
	}
	return "", fmt.Errorf("LLM classification unavailable")
}

func (o *Orchestrator) RegisterNode(ctx context.Context, req *proto.RegisterNodeRequest) (*proto.RegisterNodeResponse, error) {
	fmt.Printf("[Orchestrator] Registering node: %s (%s)\n", req.Endpoint, req.Tier)
	
	o.nodesMu.Lock()
	defer o.nodesMu.Unlock()

	o.nodes[req.Endpoint] = &registeredNode{
		status: &proto.NodeStatus{
			Endpoint:       req.Endpoint,
			Tier:           req.Tier,
			CpuUtilization: 0.0,
			CurrentSimops:  0,
			LastSeenMs:     time.Now().UnixNano() / int64(time.Millisecond),
		},
		maxSimops: req.MaxSimops,
	}

	return &proto.RegisterNodeResponse{Success: true, Message: "Node registered successfully"}, nil
}

func (o *Orchestrator) ReportHeartbeat(ctx context.Context, req *proto.HeartbeatRequest) (*proto.HeartbeatResponse, error) {
	fmt.Printf("[Orchestrator] Heartbeat from %s: CPU %.2f%%\n", req.Endpoint, req.CpuUtilization)
	
	o.nodesMu.Lock()
	defer o.nodesMu.Unlock()

	if n, ok := o.nodes[req.Endpoint]; ok {
		n.status.CurrentSimops = req.CurrentSimops
		n.status.CpuUtilization = req.CpuUtilization
		n.status.LastSeenMs = time.Now().UnixNano() / int64(time.Millisecond)
		return &proto.HeartbeatResponse{Success: true}, nil
	}

	return &proto.HeartbeatResponse{Success: false}, nil
}

func (o *Orchestrator) RequestSecret(ctx context.Context, req *proto.RequestSecretRequest) (*proto.RequestSecretResponse, error) {
	fmt.Printf("[Orchestrator] MISSION %s REQUESTING SECRET: %s for %s\n", req.MissionId, req.SecretKey, req.Site)
	
	fmt.Println("---------------------------------------------------------")
	fmt.Printf("USER AUTHORIZATION REQUIRED:\nAllow mission %s to access %s for %s? (Y/n): ", req.MissionId, req.SecretKey, req.Site)
	
	if os.Getenv("VAULT_AUTO_APPROVE") != "true" {
		return &proto.RequestSecretResponse{Authorized: false}, nil
	}
	
	fmt.Println("Y (Auto-approved)")
	fmt.Println("---------------------------------------------------------")

	val, err := o.vault.GetSecret(req.Site, req.SecretKey)
	if err != nil {
		return &proto.RequestSecretResponse{Authorized: false}, nil
	}

	return &proto.RequestSecretResponse{Authorized: true, SecretValue: val}, nil
}

func (o *Orchestrator) SubmitMission(ctx context.Context, req *proto.SubmitMissionRequest) (*proto.SubmitMissionResponse, error) {
	fmt.Printf("[Orchestrator] RECEIVED MISSION FROM INITIATOR: %s (URL: %s)\n", req.MissionId, req.Url)

	spawnReq := &proto.SpawnAgentRequest{
		Url:               req.Url,
		SystemInstruction: req.Instruction,
		MissionId:         req.MissionId,
		InstanceId:        "mobile_initiator",
	}

	nodeAddr, err := o.SelectBestNode(ctx, spawnReq)
	if err != nil {
		return &proto.SubmitMissionResponse{Accepted: false, Message: fmt.Sprintf("Failed to find node: %v", err)}, nil
	}

	fmt.Printf("[Orchestrator] Dispatching mission %s to %s...\n", req.MissionId, nodeAddr)
	
	o.missionsMu.Lock()
	o.missionStatuses[req.MissionId] = &MissionState{
		MissionID:    req.MissionId,
		Status:       "running",
		DispatchedTo: nodeAddr,
	}
	o.missionsMu.Unlock()

	go func() {
		resp, err := o.DispatchMission(context.Background(), nodeAddr, spawnReq)
		o.missionsMu.Lock()
		if err != nil {
			fmt.Printf("[Orchestrator] Mission %s failed: %v\n", req.MissionId, err)
			if ms, ok := o.missionStatuses[req.MissionId]; ok {
				ms.Status = "failed"
				ms.ResultJSON = err.Error()
			}
		} else {
			fmt.Printf("[Orchestrator] Mission %s completed. Result: %s\n", req.MissionId, resp.ResultJson)
			if ms, ok := o.missionStatuses[req.MissionId]; ok {
				ms.Status = "success"
				ms.ResultJSON = resp.ResultJson
			}
			o.RecordFlow(context.Background(), req.Url, resp.ResultJson)
		}
		o.missionsMu.Unlock()
	}()

	return &proto.SubmitMissionResponse{Accepted: true, Message: fmt.Sprintf("Mission dispatched to %s", nodeAddr)}, nil
}

func (o *Orchestrator) GetSwarmStatus(ctx context.Context, req *proto.SwarmStatusRequest) (*proto.SwarmStatusResponse, error) {
	o.nodesMu.Lock()
	var activeNodes []*proto.NodeStatus
	nowMs := time.Now().UnixNano() / int64(time.Millisecond)
	for _, n := range o.nodes {
		if n.status.LastSeenMs > nowMs - 60000 {
			activeNodes = append(activeNodes, n.status)
		}
	}
	o.nodesMu.Unlock()

	o.missionsMu.RLock()
	activeMissions := int32(0)
	for _, ms := range o.missionStatuses {
		if ms.Status == "running" {
			activeMissions++
		}
	}
	o.missionsMu.RUnlock()

	return &proto.SwarmStatusResponse{
		Nodes:          activeNodes,
		ActiveMissions: activeMissions,
	}, nil
}

func (o *Orchestrator) GetMissionResult(ctx context.Context, req *proto.MissionResultRequest) (*proto.MissionResultResponse, error) {
	o.missionsMu.RLock()
	defer o.missionsMu.RUnlock()

	ms, ok := o.missionStatuses[req.MissionId]
	if !ok {
		return &proto.MissionResultResponse{
			MissionId: req.MissionId,
			Status:    "not_found",
		}, nil
	}

	return &proto.MissionResultResponse{
		MissionId:    ms.MissionID,
		Status:       ms.Status,
		ResultJson:   ms.ResultJSON,
		DispatchedTo: ms.DispatchedTo,
	}, nil
}

func (o *Orchestrator) saveTriple(sub uint64, pred uint16, obj uint64) {
	node := &sitemap.TripleNode{
		SubjectHash: sub,
		PredicateID: pred,
		ObjectHash:  obj,
	}
	_, _ = o.sm.SaveNode(node)
}

func LoadAllTriples(basePath string) ([]sitemap.TripleNode, error) {
	dir := filepath.Join(basePath, "nodes")
	files, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}

	var triples []sitemap.TripleNode
	for _, f := range files {
		if filepath.Ext(f.Name()) != ".nda" {
			continue
		}
		data, err := os.ReadFile(filepath.Join(dir, f.Name()))
		if err != nil {
			continue
		}
		if len(data) == 19 && data[0] == 'T' {
			sub := binary.LittleEndian.Uint64(data[1:9])
			pred := binary.LittleEndian.Uint16(data[9:11])
			obj := binary.LittleEndian.Uint64(data[11:19])
			triples = append(triples, sitemap.TripleNode{
				SubjectHash: sub,
				PredicateID: pred,
				ObjectHash:  obj,
			})
		}
	}
	return triples, nil
}
