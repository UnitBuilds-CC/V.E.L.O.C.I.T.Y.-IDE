package swarm

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/chromedp/chromedp"
	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
	"github.com/reclamation-admin/agentic-browser-go/pkg/swarm/proto"
)

// NodeAgent runs on the remote machine and listens for SpawnAgent requests.
type NodeAgent struct {
	proto.UnimplementedSwarmServiceServer
	activeSimOps       int32
	orchestratorClient proto.OrchestratorServiceClient
	mu                 sync.RWMutex
}

// NewNodeAgent creates a new NodeAgent.
func NewNodeAgent() *NodeAgent {
	return &NodeAgent{}
}

// SetOrchestrator sets the active orchestrator client for callbacks.
func (n *NodeAgent) SetOrchestrator(client proto.OrchestratorServiceClient) {
	n.mu.Lock()
	defer n.mu.Unlock()
	n.orchestratorClient = client
}

// requestSecret asks the orchestrator for a sensitive value.
func (n *NodeAgent) requestSecret(ctx context.Context, missionId, site, key string) (string, error) {
	n.mu.RLock()
	client := n.orchestratorClient
	n.mu.RUnlock()

	if client == nil {
		return "", fmt.Errorf("no orchestrator connected for secret requests")
	}

	resp, err := client.RequestSecret(ctx, &proto.RequestSecretRequest{
		MissionId: missionId,
		Site:      site,
		SecretKey: key,
	})
	if err != nil {
		return "", err
	}
	if !resp.Authorized {
		return "", fmt.Errorf("secret request denied by user")
	}
	return resp.SecretValue, nil
}

// GetActiveSimOps returns the number of active missions.
func (n *NodeAgent) GetActiveSimOps() int32 {
	return atomic.LoadInt32(&n.activeSimOps)
}

// SpawnAgent implements the gRPC SwarmServiceServer interface.
func (n *NodeAgent) SpawnAgent(ctx context.Context, req *proto.SpawnAgentRequest) (*proto.SpawnAgentResponse, error) {
	atomic.AddInt32(&n.activeSimOps, 1)
	defer atomic.AddInt32(&n.activeSimOps, -1)
	
	fmt.Printf("[NodeAgent] Received request to spawn agent for mission %s\n", req.MissionId)
	
	// 1. Create a local mission file for the agent to read
	missionFile := fmt.Sprintf("mission_%s.json", req.MissionId)
	data, _ := json.Marshal(req)
	os.WriteFile(missionFile, data, 0644)
	
	fmt.Printf("[NodeAgent] Executing instructions: %s\n", req.SystemInstruction)
	
	// 2. Start real browser session
	fmt.Println("[NodeAgent] Starting real browser session...")
	session, err := browser.NewManagedSession()
	if err != nil {
		return nil, fmt.Errorf("failed to start browser: %v", err)
	}
	defer session.Close()

	// 3. Navigate to URL
	fmt.Printf("[NodeAgent] Navigating to %s...\n", req.Url)
	if err := session.Navigate(req.Url); err != nil {
		return nil, fmt.Errorf("failed to navigate: %v", err)
	}

	// Wait for load
	fmt.Println("[NodeAgent] Waiting for page to load...")
	time.Sleep(5 * time.Second)

	// 4. Take real screenshot with timeout
	fmt.Println("[NodeAgent] Capturing screenshot...")
	var buf []byte
	shotCtx, shotCancel := context.WithTimeout(session.Ctx, 10*time.Second)
	if err := chromedp.Run(shotCtx, chromedp.CaptureScreenshot(&buf)); err != nil {
		fmt.Printf("[NodeAgent] Warning: Failed to capture screenshot: %v\n", err)
	}
	shotCancel()

	// 5. Autonomous Loop
	fields := make(map[string]string)
	maxSteps := 5
	for step := 0; step < maxSteps; step++ {
		fmt.Printf("[NodeAgent] Step %d/%d...\n", step+1, maxSteps)
		
		// 5.1 Extract real fields using AOM
		fmt.Println("[NodeAgent] Extracting fields using AOM...")
		_, err = session.GetAom(browser.AomConfig{MaxLength: 95000})
		if err != nil {
			fmt.Printf("[NodeAgent] Warning: Failed to get AOM: %v\n", err)
		}
	
		fields = make(map[string]string)
		var findFields func([]*browser.PrunedNode)
		findFields = func(ns []*browser.PrunedNode) {
			for _, n := range ns {
				role := strings.ToLower(n.Role)
				// Expanded roles for better interaction
				isInteractive := role == "textbox" || role == "searchbox" || role == "combobox" || 
								 role == "checkbox" || role == "radio" || role == "button" || 
								 role == "link" || role == "menuitem" || role == "img"
				
				if isInteractive {
					key := n.Name
					if key == "" {
						key = n.Value
					}
					if key == "" {
						key = fmt.Sprintf("%s_%d", role, n.BackendID)
					}
					fields[key] = fmt.Sprintf("%d", n.BackendID)
				}
				findFields(n.Children)
			}
		}
		if len(session.LastAom) > 0 {
			findFields(session.LastAom)
		}
	
		// 5.2 Call Small LLM
		action, target, text, err := n.callSmallLLM(ctx, req.SystemInstruction, fields)
		if err != nil {
			fmt.Printf("[NodeAgent] LLM failed: %v\n", err)
			break
		}
		
		fmt.Printf("[NodeAgent] LLM decided: Action=%s, Target=%s, Text=%s\n", action, target, text)
		
		// 5.3 Execute Action
		switch action {
		case "COMPLETE":
			fmt.Println("[NodeAgent] Mission complete!")
			goto Done
		case "CLICK":
			err = session.Click(target)
		case "TYPE":
			err = session.TypeText(target, text)
		case "SCROLL":
			amount := 500
			if strings.ToLower(text) == "up" {
				err = session.Scroll("up", amount)
			} else {
				err = session.Scroll("down", amount)
			}
		case "NAVIGATE":
			fmt.Printf("[NodeAgent] Navigating to %s...\n", text)
			err = session.Navigate(text)
		case "WAIT":
			seconds := 2
			fmt.Sscanf(text, "%d", &seconds)
			fmt.Printf("[NodeAgent] Waiting for %d seconds...\n", seconds)
			time.Sleep(time.Duration(seconds) * time.Second)
		case "REQUEST_SECRET":
			fmt.Printf("[NodeAgent] Requesting secret %s for %s...\n", text, req.Url)
			// Extract domain from URL
			domain := req.Url
			if strings.Contains(domain, "://") {
				parts := strings.Split(domain, "://")
				domain = parts[1]
			}
			if strings.Contains(domain, "/") {
				domain = strings.Split(domain, "/")[0]
			}

			val, errSecret := n.requestSecret(ctx, req.MissionId, domain, text)
			if errSecret != nil {
				fmt.Printf("[NodeAgent] Secret request failed: %v\n", errSecret)
				err = errSecret
				break
			}
			fmt.Printf("[NodeAgent] Secret received. Typing into %s...\n", target)
			err = session.TypeText(target, val)
		default:
			fmt.Printf("[NodeAgent] Unknown action: %s\n", action)
		}
		
		if err != nil {
			fmt.Printf("[NodeAgent] Action failed: %v\n", err)
			break
		}
		
		if action == "COMPLETE" {
			break
		}
		
		// Wait for page to settle
		time.Sleep(2 * time.Second)
	}

Done:
	// 6. Detect failures
	fmt.Println("[NodeAgent] Detecting failures...")
	var title string
	if err := chromedp.Run(session.Ctx, chromedp.Title(&title)); err != nil {
		fmt.Printf("[NodeAgent] Warning: Failed to get title: %v\n", err)
	}

	status := "success"
	reason := ""
	
	if strings.Contains(title, "Access Denied") || strings.Contains(title, "403 Forbidden") {
		status = "failed"
		reason = "Access Denied"
	} else if strings.Contains(title, "Just a moment") || strings.Contains(title, "Cloudflare") {
		status = "failed"
		reason = "Cloudflare Challenge"
	} else if strings.Contains(title, "DataDome") {
		status = "failed"
		reason = "DataDome Challenge"
	}
	
	if status == "success" {
		text := session.GetPageText()
		if strings.Contains(text, "hCaptcha") || strings.Contains(text, "reCAPTCHA") {
			status = "failed"
			reason = "Captcha Detected"
		}
	}

	resultObj := map[string]interface{}{
		"status":   status,
		"anti_bot": reason,
		"reason":   reason,
		"fields":   fields,
		"url":      req.Url,
	}
	resultJsonBytes, _ := json.Marshal(resultObj)
	resultJson := string(resultJsonBytes)
	
	return &proto.SpawnAgentResponse{
		Success:    true,
		Message:    "Agent completed task with real browser and fields",
		ResultJson: resultJson,
		Screenshot: buf,
	}, nil
}

// loadSkills reads all markdown files from the skills/library directory.
func (n *NodeAgent) loadSkills() string {
	files, err := os.ReadDir("skills/library")
	if err != nil {
		fmt.Printf("[NodeAgent] Error reading skills library: %v\n", err)
		return "You are a browser agent. Use JSON to respond."
	}

	var builder strings.Builder
	builder.WriteString("=== SYSTEM INSTRUCTIONS ===\n")
	
	// Ensure base.md is loaded first if it exists
	if data, err := os.ReadFile("skills/library/base.md"); err == nil {
		builder.Write(data)
		builder.WriteString("\n\n")
	}

	for _, file := range files {
		if file.Name() == "base.md" || !strings.HasSuffix(file.Name(), ".md") {
			continue
		}
		data, err := os.ReadFile("skills/library/" + file.Name())
		if err == nil {
			builder.WriteString(fmt.Sprintf("--- Skillset: %s ---\n", file.Name()))
			builder.Write(data)
			builder.WriteString("\n\n")
		}
	}
	return builder.String()
}


// callSmallLLM calls LM Studio to decide the next action.
func (n *NodeAgent) callSmallLLM(ctx context.Context, mission string, fields map[string]string) (string, string, string, error) {
	skills := n.loadSkills()
	fieldsJson, _ := json.Marshal(fields)

	if os.Getenv("DEBUG_MODE") == "true" {
		fmt.Println("[NodeAgent] DEBUG MODE: Writing prompt to debug_prompt.txt...")
		
		prompt := fmt.Sprintf("%s\n\n=== Mission ===\n%s\n\n=== Available Fields ===\n%s", skills, mission, string(fieldsJson))
		os.WriteFile("debug_prompt.txt", []byte(prompt), 0644)
		
		fmt.Println("[NodeAgent] DEBUG MODE: Waiting for debug_response.json...")
		for i := 0; i < 300; i++ {
			data, err := os.ReadFile("debug_response.json")
			if err == nil {
				var actionObj struct {
					Action string `json:"action"`
					Target string `json:"target"`
					Text   string `json:"text"`
				}
				if jsonErr := json.Unmarshal(data, &actionObj); jsonErr == nil {
					os.Remove("debug_response.json") // Clean up
					return actionObj.Action, actionObj.Target, actionObj.Text, nil
				} else {
					fmt.Printf("[NodeAgent] DEBUG MODE: Failed to parse JSON: %v\n", jsonErr)
				}
			}
			time.Sleep(1 * time.Second)
		}
		return "", "", "", fmt.Errorf("timeout waiting for debug_response.json")
	}

	url := os.Getenv("LM_STUDIO_URL")
	if url == "" {
		url = "http://localhost:1234/v1"
	}

	payload := map[string]interface{}{
		"model": "local-model",
		"messages": []map[string]string{
			{
				"role":    "system",
				"content": skills,
			},
			{
				"role":    "user",
				"content": fmt.Sprintf("Mission: %s\nAvailable fields: %s\n\nRESPONSE REQUIREMENT:\nYou MUST respond with ONLY a single JSON object. No reasoning, no chatter. \nFormat: {\"action\": \"...\", \"target\": \"...\", \"text\": \"...\"}", mission, string(fieldsJson)),
			},
		},
		"temperature": 0.0,
	}

	jsonBytes, _ := json.Marshal(payload)
	req, err := http.NewRequestWithContext(ctx, "POST", url+"/chat/completions", bytes.NewBuffer(jsonBytes))
	if err != nil {
		return "", "", "", err
	}
	req.Header.Set("Content-Type", "application/json")

	client := &http.Client{Timeout: 180 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return "", "", "", err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return "", "", "", fmt.Errorf("LM Studio returned status: %d", resp.StatusCode)
	}

	var result map[string]interface{}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return "", "", "", err
	}

	choices, ok := result["choices"].([]interface{})
	if !ok || len(choices) == 0 {
		return "", "", "", fmt.Errorf("invalid response")
	}
	firstChoice := choices[0].(map[string]interface{})
	message := firstChoice["message"].(map[string]interface{})
	content := message["content"].(string)

	var actionObj struct {
		Action string `json:"action"`
		Target string `json:"target"`
		Text   string `json:"text"`
	}
	if err := json.Unmarshal([]byte(content), &actionObj); err != nil {
		return "", "", "", fmt.Errorf("failed to parse action JSON: %v. Content was: %s", err, content)
	}

	return actionObj.Action, actionObj.Target, actionObj.Text, nil
}
