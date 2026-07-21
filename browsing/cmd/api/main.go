package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/chromedp/cdproto/network"
	"github.com/chromedp/chromedp"
	"github.com/gin-gonic/gin"
	browserpkg "github.com/reclamation-admin/agentic-browser-go/pkg/browser"
	"github.com/reclamation-admin/agentic-browser-go/pkg/db"
)

type GenerateRequest struct {
	StartURL    string `json:"startUrl"`
	TargetLabel string `json:"targetLabel"`
}

type runtimeSessionStore struct {
	mu       sync.RWMutex
	sessions map[string]*runtimeSessionEntry
}

type runtimeSessionEntry struct {
	ID         string
	Mode       string
	DebugPort  int
	CreatedAt  time.Time
	LastAction string
	Session    *browserpkg.Session
}

type runtimeOpenSessionRequest struct {
	DebugPort     int    `json:"debugPort"`
	StartURL      string `json:"startUrl"`
	WaitTimeoutMs int    `json:"waitTimeoutMs"`
}

type runtimeSessionActionRequest struct {
	Action        string `json:"action"`
	NodeID        string `json:"nodeId"`
	Selector      string `json:"selector"`
	Value         string `json:"value"`
	Key           string `json:"key"`
	URL           string `json:"url"`
	Script        string `json:"script"`
	Natural       bool   `json:"natural"`
	Clear         bool   `json:"clear"`
	WaitTimeoutMs int    `json:"waitTimeoutMs"`
}

type runtimeSessionState struct {
	SessionID       string    `json:"sessionId,omitempty"`
	Alive           bool      `json:"alive"`
	Mode            string    `json:"mode"`
	DebugPort       int       `json:"debugPort,omitempty"`
	CreatedAt       time.Time `json:"createdAt"`
	LastAction      string    `json:"lastAction,omitempty"`
	ActiveTarget    string    `json:"activeTargetId,omitempty"`
	MainTarget      string    `json:"mainTargetId,omitempty"`
	LastAomNodes    int       `json:"lastAomNodeCount"`
	FrameCount      int       `json:"frameCount,omitempty"`
	ShadowHostCount int       `json:"shadowHostCount,omitempty"`
}

type runtimeFrameSummary struct {
	Selector          string `json:"selector,omitempty"`
	Name              string `json:"name,omitempty"`
	Title             string `json:"title,omitempty"`
	Source            string `json:"source,omitempty"`
	SameOrigin        bool   `json:"sameOrigin"`
	Accessible        bool   `json:"accessible"`
	SemanticNodeCount int    `json:"semanticNodeCount,omitempty"`
}

type runtimeShadowHostSummary struct {
	Selector          string `json:"selector,omitempty"`
	Tag               string `json:"tag,omitempty"`
	Role              string `json:"role,omitempty"`
	Mode              string `json:"mode,omitempty"`
	SemanticNodeCount int    `json:"semanticNodeCount,omitempty"`
	TextSample        string `json:"textSample,omitempty"`
}

type runtimeProtocolEvidence struct {
	Backend          string   `json:"backend"`
	Transport        string   `json:"transport"`
	SessionMode      string   `json:"sessionMode"`
	SupportsActions  []string `json:"supportsActions"`
	SupportsCapture  bool     `json:"supportsCapture"`
	SupportsSessions bool     `json:"supportsSessions"`
}

type runtimeStorageSnapshot struct {
	Local   map[string]string `json:"local"`
	Session map[string]string `json:"session"`
}

type runtimeActionResult struct {
	Action        string   `json:"action"`
	Target        string   `json:"target,omitempty"`
	Value         string   `json:"value,omitempty"`
	Key           string   `json:"key,omitempty"`
	Script        string   `json:"script,omitempty"`
	Result        string   `json:"result,omitempty"`
	WaitAppliedMs int      `json:"waitAppliedMs"`
	Warnings      []string `json:"warnings,omitempty"`
}

type runtimeSessionCaptureResponse struct {
	SessionID        string                     `json:"sessionId,omitempty"`
	FinalURL         string                     `json:"finalUrl"`
	Title            string                     `json:"title"`
	HTML             string                     `json:"html"`
	Cookies          []string                   `json:"cookies"`
	Storage          runtimeStorageSnapshot     `json:"storage"`
	Fields           map[string]string          `json:"fields"`
	Frames           []runtimeFrameSummary      `json:"frames,omitempty"`
	ShadowHosts      []runtimeShadowHostSummary `json:"shadowHosts,omitempty"`
	RuntimeState     runtimeSessionState        `json:"runtimeState"`
	ProtocolEvidence runtimeProtocolEvidence    `json:"protocolEvidence"`
	Warnings         []string                   `json:"warnings,omitempty"`
	Action           *runtimeActionResult       `json:"action,omitempty"`
	AOM              string                     `json:"aom,omitempty"`
	PageText         string                     `json:"pageText,omitempty"`
	Scripts          []string                   `json:"scripts,omitempty"`
}

type runtimeOpenSessionResponse struct {
	SessionID        string                  `json:"sessionId"`
	RuntimeState     runtimeSessionState     `json:"runtimeState"`
	ProtocolEvidence runtimeProtocolEvidence `json:"protocolEvidence"`
	Warnings         []string                `json:"warnings,omitempty"`
}

type RuntimeCaptureRequest struct {
	URL       string `json:"url"`
	TimeoutMs int64  `json:"timeout_ms"`
}

type RuntimeCaptureCookie struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}

type RuntimeCaptureRequestRecord struct {
	Method     string `json:"method"`
	URL        string `json:"url"`
	StatusCode uint16 `json:"status_code"`
	Resource   string `json:"resource"`
}

type RuntimeCaptureState struct {
	Scope string `json:"scope"`
	Key   string `json:"key"`
	Value string `json:"value"`
}

type RuntimeCaptureProtocolEvent struct {
	Kind   string `json:"kind"`
	Phase  string `json:"phase"`
	Target string `json:"target"`
	Detail string `json:"detail"`
}

type RuntimeCaptureResponse struct {
	FinalURL       string                        `json:"final_url"`
	Title          string                        `json:"title"`
	HTML           string                        `json:"html"`
	AomSummary     string                        `json:"aom_summary"`
	PageText       string                        `json:"page_text"`
	Scripts        []string                      `json:"scripts"`
	Fields         map[string]string             `json:"fields"`
	Cookies        []RuntimeCaptureCookie        `json:"cookies"`
	LocalStorage   map[string]string             `json:"local_storage"`
	SessionStorage map[string]string             `json:"session_storage"`
	SettleSignals  []string                      `json:"settle_signals"`
	RuntimeState   []RuntimeCaptureState         `json:"runtime_state"`
	ProtocolEvents []RuntimeCaptureProtocolEvent `json:"protocol_events"`
	Requests       []RuntimeCaptureRequestRecord `json:"requests"`
	Warnings       []string                      `json:"warnings"`
}

type RuntimeVisualArtifactRequest struct {
	URL string `json:"url"`
}

type runtimeBrowser interface {
	Navigate(url string) error
	WaitForStability(timeout time.Duration) error
	CaptureScreenshot() ([]byte, error)
	CurrentURL() (string, error)
	Close()
}

type runtimeBrowserFactory func() (runtimeBrowser, error)

var runtimeSessionSeq uint64

var (
	openRuntimeSessionFn    = openRuntimeSession
	captureRuntimeSessionFn = captureRuntimeSession
	performRuntimeActionFn  = performRuntimeAction
)

func main() {
	fmt.Println("Go Agentic API starting (SiteMap/NDA Mode)...")

	client, err := db.NewClient()
	if err != nil {
		log.Fatalf("Failed to open SiteMap database: %v", err)
	}
	defer client.Close(context.Background())

	r := buildRouter(func() (runtimeBrowser, error) {
		return browserpkg.NewManagedSession()
	})

	fmt.Println("API listening on :8080")
	if err := r.Run(":8080"); err != nil {
		log.Fatalf("API server failed: %v", err)
	}
}

func buildRouter(newRuntimeBrowser runtimeBrowserFactory) *gin.Engine {
	runtimeSessions := &runtimeSessionStore{sessions: make(map[string]*runtimeSessionEntry)}
	r := gin.Default()

	r.Use(func(c *gin.Context) {
		c.Writer.Header().Set("Access-Control-Allow-Origin", "*")
		c.Writer.Header().Set("Access-Control-Allow-Credentials", "true")
		c.Writer.Header().Set("Access-Control-Allow-Headers", "Content-Type, Content-Length, Accept-Encoding, X-CSRF-Token, Authorization, accept, origin, Cache-Control, X-Requested-With")
		c.Writer.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS, GET, PUT, DELETE")

		if c.Request.Method == "OPTIONS" {
			c.AbortWithStatus(204)
			return
		}
		c.Next()
	})

	r.GET("/api/graph/config", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"serverUrl": "sitemap_db",
			"user":      "",
			"pass":      "",
		})
	})

	r.POST("/api/graph/wipe", func(c *gin.Context) {
		_ = os.RemoveAll("sitemap_db")
		_ = os.MkdirAll("sitemap_db/nodes", 0755)
		c.JSON(http.StatusOK, gin.H{"status": "wiped"})
	})

	summaryDir := "./data/summaries"
	_ = os.MkdirAll(summaryDir, 0755)
	r.StaticFS("/api/summaries", http.Dir(summaryDir))

	r.POST("/api/flows/generate", func(c *gin.Context) {
		var req GenerateRequest
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		script, warnings, err := buildFlowScript(req)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		c.JSON(http.StatusOK, gin.H{
			"script":   script,
			"strategy": "label-driven",
			"warnings": warnings,
		})
	})

	r.POST("/api/runtime/session", func(c *gin.Context) {
		var req runtimeOpenSessionRequest
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		entry, warnings, err := openRuntimeSessionFn(req)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		runtimeSessions.put(entry)
		c.JSON(http.StatusOK, runtimeOpenSessionResponse{
			SessionID:        entry.ID,
			RuntimeState:     runtimeStateFromEntry(entry),
			ProtocolEvidence: protocolEvidenceFromEntry(entry),
			Warnings:         warnings,
		})
	})

	r.DELETE("/api/runtime/session/:sessionId", func(c *gin.Context) {
		sessionID := strings.TrimSpace(c.Param("sessionId"))
		if sessionID == "" {
			c.JSON(http.StatusBadRequest, gin.H{"error": "sessionId is required"})
			return
		}

		entry, ok := runtimeSessions.delete(sessionID)
		if !ok {
			c.JSON(http.StatusNotFound, gin.H{"error": "runtime session not found"})
			return
		}
		entry.Session.Close()
		c.JSON(http.StatusOK, gin.H{"sessionId": sessionID, "status": "closed"})
	})

	r.POST("/api/runtime/session/:sessionId/capture", func(c *gin.Context) {
		sessionID := strings.TrimSpace(c.Param("sessionId"))
		entry, ok := runtimeSessions.get(sessionID)
		if !ok {
			c.JSON(http.StatusNotFound, gin.H{"error": "runtime session not found"})
			return
		}

		resp, err := captureRuntimeSessionFn(entry)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
		c.JSON(http.StatusOK, resp)
	})

	r.POST("/api/runtime/session/:sessionId/action", func(c *gin.Context) {
		sessionID := strings.TrimSpace(c.Param("sessionId"))
		entry, ok := runtimeSessions.get(sessionID)
		if !ok {
			c.JSON(http.StatusNotFound, gin.H{"error": "runtime session not found"})
			return
		}

		var req runtimeSessionActionRequest
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		actionResult, err := performRuntimeActionFn(entry, req)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		resp, err := captureRuntimeSessionFn(entry)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
		resp.Action = actionResult
		c.JSON(http.StatusOK, resp)
	})

	r.POST("/api/runtime/capture", func(c *gin.Context) {
		var req RuntimeCaptureRequest
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		response, err := captureRuntime(req)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		c.JSON(http.StatusOK, response)
	})

	r.POST("/api/runtime/visual-artifact", func(c *gin.Context) {
		handleRuntimeVisualArtifact(c, newRuntimeBrowser)
	})

	r.GET("/", func(c *gin.Context) {
		c.Data(http.StatusOK, "text/html; charset=utf-8", []byte("<h1>Go Agentic RPA Studio</h1>"))
	})

	return r
}

func captureRuntime(req RuntimeCaptureRequest) (*RuntimeCaptureResponse, error) {
	captureURL := strings.TrimSpace(req.URL)
	if captureURL == "" {
		return nil, fmt.Errorf("url is required")
	}
	if _, err := url.ParseRequestURI(captureURL); err != nil {
		return nil, fmt.Errorf("url must be a valid absolute URL: %w", err)
	}
	if req.TimeoutMs < 0 {
		return nil, fmt.Errorf("timeout_ms must be >= 0")
	}

	timeout := 5 * time.Second
	if req.TimeoutMs > 0 {
		timeout = time.Duration(req.TimeoutMs) * time.Millisecond
	}

	session, err := browserpkg.NewManagedSession()
	if err != nil {
		return nil, fmt.Errorf("start runtime session: %w", err)
	}
	defer session.Close()

	if err := session.Navigate(captureURL); err != nil {
		return nil, fmt.Errorf("navigate runtime session: %w", err)
	}

	warnings := make([]string, 0, 4)
	if err := session.WaitForStability(timeout); err != nil {
		warnings = append(warnings, fmt.Sprintf("stability wait failed: %v", err))
	}

	aomSummary := ""
	if value, err := session.GetAom(browserpkg.AomConfig{MaxLength: 12000}); err == nil {
		aomSummary = value
	} else {
		warnings = append(warnings, fmt.Sprintf("aom capture failed: %v", err))
	}
	pageText := session.GetPageText()
	scripts := session.GetScripts()

	fields := map[string]string{}
	if value, err := session.ExtractFields(); err == nil {
		fields = value
	} else {
		warnings = append(warnings, fmt.Sprintf("field extraction failed: %v", err))
	}

	cookies, err := readRuntimeCookies(session)
	if err != nil {
		warnings = append(warnings, fmt.Sprintf("cookie capture failed: %v", err))
	}

	localStorage, sessionStorage, err := readRuntimeStorage(session)
	if err != nil {
		warnings = append(warnings, fmt.Sprintf("storage capture failed: %v", err))
	}

	var title string
	var finalURL string
	var html string
	if err := chromedp.Run(
		session.Ctx,
		chromedp.Title(&title),
		chromedp.Location(&finalURL),
		chromedp.Evaluate(`document.documentElement ? document.documentElement.outerHTML : ""`, &html),
	); err != nil {
		return nil, fmt.Errorf("read runtime page state: %w", err)
	}

	if title == "" {
		title = "Untitled Page"
	}
	if finalURL == "" {
		finalURL = captureURL
	}

	runtimeState := []RuntimeCaptureState{
		{Scope: "runtime", Key: "backend", Value: "go-chromedp"},
		{Scope: "runtime", Key: "capture_mode", Value: "live"},
		{Scope: "page", Key: "title", Value: title},
		{Scope: "page", Key: "page_text_chars", Value: fmt.Sprintf("%d", len(pageText))},
		{Scope: "page", Key: "external_script_count", Value: fmt.Sprintf("%d", len(scripts))},
		{Scope: "page", Key: "field_count", Value: fmt.Sprintf("%d", len(fields))},
	}
	if aomSummary != "" {
		runtimeState = append(runtimeState, RuntimeCaptureState{Scope: "aom", Key: "summary_chars", Value: fmt.Sprintf("%d", len(aomSummary))})
	}

	protocolEvents := []RuntimeCaptureProtocolEvent{
		{Kind: "navigation", Phase: "committed", Target: finalURL, Detail: captureURL},
		{Kind: "runtime", Phase: "stable", Target: finalURL, Detail: fmt.Sprintf("timeout_ms=%d", timeout.Milliseconds())},
	}
	if aomSummary != "" {
		protocolEvents = append(protocolEvents, RuntimeCaptureProtocolEvent{Kind: "aom", Phase: "captured", Target: finalURL, Detail: fmt.Sprintf("chars=%d", len(aomSummary))})
	}

	return &RuntimeCaptureResponse{
		FinalURL:       finalURL,
		Title:          title,
		HTML:           html,
		AomSummary:     aomSummary,
		PageText:       pageText,
		Scripts:        scripts,
		Fields:         fields,
		Cookies:        cookies,
		LocalStorage:   localStorage,
		SessionStorage: sessionStorage,
		SettleSignals:  []string{"navigation:committed", "runtime:stable"},
		RuntimeState:   runtimeState,
		ProtocolEvents: protocolEvents,
		Requests:       []RuntimeCaptureRequestRecord{},
		Warnings:       warnings,
	}, nil
}

func (s *runtimeSessionStore) get(id string) (*runtimeSessionEntry, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	entry, ok := s.sessions[id]
	return entry, ok
}

func (s *runtimeSessionStore) put(entry *runtimeSessionEntry) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.sessions[entry.ID] = entry
}

func (s *runtimeSessionStore) delete(id string) (*runtimeSessionEntry, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry, ok := s.sessions[id]
	if ok {
		delete(s.sessions, id)
	}
	return entry, ok
}

func openRuntimeSession(req runtimeOpenSessionRequest) (*runtimeSessionEntry, []string, error) {
	var session *browserpkg.Session
	var err error
	warnings := make([]string, 0, 2)

	if req.DebugPort < 0 {
		return nil, nil, fmt.Errorf("debugPort must be non-negative")
	}
	if req.WaitTimeoutMs < 0 {
		return nil, nil, fmt.Errorf("waitTimeoutMs must be non-negative")
	}

	mode := "managed"
	if req.DebugPort > 0 {
		mode = "remote"
		session, err = browserpkg.NewSession(req.DebugPort)
	} else {
		session, err = browserpkg.NewManagedSession()
	}
	if err != nil {
		return nil, nil, fmt.Errorf("failed to open runtime session: %w", err)
	}

	entry := &runtimeSessionEntry{
		ID:         fmt.Sprintf("rt-%d", atomic.AddUint64(&runtimeSessionSeq, 1)),
		Mode:       mode,
		DebugPort:  req.DebugPort,
		CreatedAt:  time.Now().UTC(),
		Session:    session,
		LastAction: "open",
	}

	if req.StartURL != "" {
		if err := validateAbsoluteURL(req.StartURL); err != nil {
			session.Close()
			return nil, nil, err
		}
		if err := session.Navigate(req.StartURL); err != nil {
			session.Close()
			return nil, nil, fmt.Errorf("failed to navigate runtime session: %w", err)
		}
		waitMs := normalizeWait(req.WaitTimeoutMs, 2000)
		if err := session.QuickWait(time.Duration(waitMs) * time.Millisecond); err != nil {
			warnings = append(warnings, fmt.Sprintf("post-navigation wait did not settle cleanly: %v", err))
		}
		entry.LastAction = "navigate"
	}

	return entry, warnings, nil
}

func captureRuntimeSession(entry *runtimeSessionEntry) (*runtimeSessionCaptureResponse, error) {
	if entry == nil || entry.Session == nil {
		return nil, fmt.Errorf("runtime session is not available")
	}
	if !entry.Session.IsAlive() {
		return nil, fmt.Errorf("runtime session is no longer alive")
	}

	warnings := make([]string, 0, 8)
	waitMs := 1500
	if err := entry.Session.QuickWait(time.Duration(waitMs) * time.Millisecond); err != nil {
		warnings = append(warnings, fmt.Sprintf("pre-capture stability wait did not settle cleanly: %v", err))
	}

	var title string
	var finalURL string
	var html string
	if err := chromedp.Run(
		entry.Session.Ctx,
		chromedp.Title(&title),
		chromedp.Location(&finalURL),
		chromedp.OuterHTML("html", &html, chromedp.ByQuery),
	); err != nil {
		return nil, fmt.Errorf("failed to capture page metadata: %w", err)
	}

	storage, err := fetchStorage(entry.Session)
	if err != nil {
		warnings = append(warnings, fmt.Sprintf("failed to capture storage: %v", err))
		storage = runtimeStorageSnapshot{Local: map[string]string{}, Session: map[string]string{}}
	}

	aom, err := entry.Session.GetAom(browserpkg.AomConfig{WithSpatial: true, MaxLength: 20000, Summarized: true})
	if err != nil {
		warnings = append(warnings, fmt.Sprintf("failed to capture AOM: %v", err))
		aom = ""
	}

	fields, err := entry.Session.ExtractFields()
	if err != nil {
		warnings = append(warnings, fmt.Sprintf("failed to extract fields: %v", err))
		fields = map[string]string{}
	}
	frames, shadowHosts, err := captureFrameAndShadowInventory(entry.Session)
	if err != nil {
		warnings = append(warnings, fmt.Sprintf("failed to inventory frames/shadow hosts: %v", err))
		frames = nil
		shadowHosts = nil
	}

	state := runtimeStateFromEntry(entry)
	state.FrameCount = len(frames)
	state.ShadowHostCount = len(shadowHosts)
	resp := &runtimeSessionCaptureResponse{
		SessionID:        entry.ID,
		FinalURL:         finalURL,
		Title:            title,
		HTML:             truncateWithWarning("html", html, 100000, &warnings),
		Cookies:          entry.Session.GetCookies(),
		Storage:          storage,
		Fields:           fields,
		Frames:           frames,
		ShadowHosts:      shadowHosts,
		RuntimeState:     state,
		ProtocolEvidence: protocolEvidenceFromEntry(entry),
		Warnings:         warnings,
		AOM:              truncateWithWarning("aom", aom, 20000, &warnings),
		PageText:         truncateWithWarning("pageText", entry.Session.GetPageText(), 20000, &warnings),
		Scripts:          entry.Session.GetScripts(),
	}
	return resp, nil
}

func fetchStorage(session *browserpkg.Session) (runtimeStorageSnapshot, error) {
	result := runtimeStorageSnapshot{Local: map[string]string{}, Session: map[string]string{}}
	script := `(function() {
		const toMap = (store) => {
			const out = {};
			for (let i = 0; i < store.length; i++) {
				const key = store.key(i);
				out[key] = store.getItem(key);
			}
			return out;
		};
		return {
			local: toMap(window.localStorage),
			session: toMap(window.sessionStorage)
		};
	})()`
	if err := chromedp.Run(session.Ctx, chromedp.Evaluate(script, &result)); err != nil {
		return result, err
	}
	return result, nil
}

func performRuntimeAction(entry *runtimeSessionEntry, req runtimeSessionActionRequest) (*runtimeActionResult, error) {
	if entry == nil || entry.Session == nil {
		return nil, fmt.Errorf("runtime session is not available")
	}
	if !entry.Session.IsAlive() {
		return nil, fmt.Errorf("runtime session is no longer alive")
	}

	action := strings.ToLower(strings.TrimSpace(req.Action))
	if action == "" {
		return nil, fmt.Errorf("action is required")
	}
	if req.WaitTimeoutMs < 0 {
		return nil, fmt.Errorf("waitTimeoutMs must be non-negative")
	}

	result := &runtimeActionResult{Action: action, WaitAppliedMs: normalizeWait(req.WaitTimeoutMs, 1500)}
	var err error

	switch action {
	case "click":
		result.Target, err = resolveActionTarget(req.NodeID, req.Selector)
		if err == nil {
			err = entry.Session.Click(result.Target)
		}
	case "js_click":
		if strings.TrimSpace(req.Selector) != "" {
			return nil, fmt.Errorf("js_click requires nodeId; selector fallback is not supported")
		}
		result.Target, err = resolveActionTarget(req.NodeID, "")
		if err == nil {
			err = entry.Session.JSClick(result.Target)
		}
	case "fill":
		result.Target, err = resolveActionTarget(req.NodeID, req.Selector)
		if err != nil {
			break
		}
		result.Value = req.Value
		if strings.TrimSpace(req.Value) == "" {
			return nil, fmt.Errorf("value is required for fill")
		}
		if req.Clear {
			if err = entry.Session.Click(result.Target); err == nil {
				err = entry.Session.PressKey("Ctrl+A")
			}
			if err == nil {
				err = entry.Session.PressKey("Delete")
			}
		}
		if err == nil {
			if req.Natural {
				err = entry.Session.TypeNatural(result.Target, req.Value)
			} else {
				err = entry.Session.TypeText(result.Target, req.Value)
			}
		}
	case "submit":
		result.Target = strings.TrimSpace(req.NodeID)
		if result.Target == "" {
			result.Target = strings.TrimSpace(req.Selector)
		}
		if result.Target != "" {
			err = entry.Session.Click(result.Target)
		} else {
			err = entry.Session.PressKey("Enter")
		}
	case "press_key":
		result.Key = strings.TrimSpace(req.Key)
		if result.Key == "" {
			return nil, fmt.Errorf("key is required for press_key")
		}
		err = entry.Session.PressKey(result.Key)
	case "navigate":
		result.Value = strings.TrimSpace(req.URL)
		if err = validateAbsoluteURL(result.Value); err != nil {
			return nil, err
		}
		err = entry.Session.Navigate(result.Value)
	case "evaluate":
		result.Script = strings.TrimSpace(req.Script)
		if result.Script == "" {
			return nil, fmt.Errorf("script is required for evaluate")
		}
		var evalResult interface{}
		err = chromedp.Run(entry.Session.Ctx, chromedp.Evaluate(result.Script, &evalResult))
		if err == nil {
			encoded, marshalErr := json.Marshal(evalResult)
			if marshalErr != nil {
				return nil, fmt.Errorf("marshal evaluate result: %w", marshalErr)
			}
			result.Result = string(encoded)
		}
	default:
		return nil, fmt.Errorf("unsupported runtime action %q", action)
	}

	if err != nil {
		return nil, fmt.Errorf("runtime action %q failed: %w", action, err)
	}

	entry.LastAction = action
	if waitErr := entry.Session.QuickWait(time.Duration(result.WaitAppliedMs) * time.Millisecond); waitErr != nil {
		result.Warnings = append(result.Warnings, fmt.Sprintf("post-action wait did not settle cleanly: %v", waitErr))
	}
	return result, nil
}

func resolveActionTarget(nodeID string, selector string) (string, error) {
	target := strings.TrimSpace(nodeID)
	if target != "" {
		return target, nil
	}
	target = strings.TrimSpace(selector)
	if target != "" {
		return target, nil
	}
	return "", fmt.Errorf("either nodeId or selector is required")
}

func runtimeStateFromEntry(entry *runtimeSessionEntry) runtimeSessionState {
	state := runtimeSessionState{}
	if entry == nil {
		return state
	}
	state.SessionID = entry.ID
	state.Mode = entry.Mode
	state.DebugPort = entry.DebugPort
	state.CreatedAt = entry.CreatedAt
	state.LastAction = entry.LastAction
	if entry.Session != nil {
		state.Alive = entry.Session.IsAlive()
		state.ActiveTarget = string(entry.Session.ActiveTargetID)
		state.MainTarget = string(entry.Session.MainTargetID)
		state.LastAomNodes = countAomNodes(entry.Session.LastAom)
	}
	return state
}

func captureFrameAndShadowInventory(session *browserpkg.Session) ([]runtimeFrameSummary, []runtimeShadowHostSummary, error) {
	if session == nil {
		return nil, nil, fmt.Errorf("session is nil")
	}
	var result struct {
		Frames      []runtimeFrameSummary      `json:"frames"`
		ShadowHosts []runtimeShadowHostSummary `json:"shadowHosts"`
	}
	const inventoryScript = `(function() {
		const cssEscape = (value) => {
			if (typeof value !== 'string') return '';
			if (window.CSS && typeof window.CSS.escape === 'function') return window.CSS.escape(value);
			return value.replace(/[^a-zA-Z0-9_-]/g, '\\$&');
		};
		const selectorFor = (el) => {
			if (!el || el.nodeType !== Node.ELEMENT_NODE) return '';
			const tag = (el.tagName || '').toLowerCase();
			if (!tag) return '';
			if (el.id) return tag + '#' + cssEscape(el.id);
			if (el.getAttribute) {
				const name = el.getAttribute('name');
				if (name) return tag + '[name="' + String(name).replace(/"/g, '\\"') + '"]';
			}
			const parent = el.parentElement;
			if (!parent) return tag;
			const siblings = Array.from(parent.children).filter((child) => child.tagName === el.tagName);
			if (siblings.length <= 1) return tag;
			return tag + ':nth-of-type(' + (siblings.indexOf(el) + 1) + ')';
		};
		const semanticSelector = 'a,button,input,select,textarea,[role],summary,label';
		const frames = Array.from(document.querySelectorAll('iframe,frame')).map((el) => {
			let accessible = false;
			let sameOrigin = false;
			let semanticNodeCount = 0;
			let title = '';
			try {
				const doc = el.contentDocument;
				accessible = !!doc;
				sameOrigin = !!doc;
				if (doc) {
					semanticNodeCount = doc.querySelectorAll(semanticSelector).length;
					title = (doc.title || '').trim();
				}
			} catch (err) {
				accessible = false;
				sameOrigin = false;
			}
			return {
				selector: selectorFor(el),
				name: el.getAttribute('name') || '',
				title,
				source: el.getAttribute('src') || '',
				sameOrigin,
				accessible,
				semanticNodeCount,
			};
		});
		const shadowHosts = [];
		const all = Array.from(document.querySelectorAll('*'));
		for (const el of all) {
			if (!el.shadowRoot) continue;
			shadowHosts.push({
				selector: selectorFor(el),
				tag: (el.tagName || '').toLowerCase(),
				role: el.getAttribute('role') || '',
				mode: 'open',
				semanticNodeCount: el.shadowRoot.querySelectorAll(semanticSelector).length,
				textSample: ((el.shadowRoot.innerText || el.shadowRoot.textContent || '').trim()).slice(0, 120),
			});
		}
		return { frames, shadowHosts };
	})()`
	if err := chromedp.Run(session.Ctx, chromedp.Evaluate(inventoryScript, &result)); err != nil {
		return nil, nil, err
	}
	return result.Frames, result.ShadowHosts, nil
}

func protocolEvidenceFromEntry(entry *runtimeSessionEntry) runtimeProtocolEvidence {
	mode := "managed"
	if entry != nil && entry.Mode != "" {
		mode = entry.Mode
	}
	return runtimeProtocolEvidence{
		Backend:          "go-chromedp",
		Transport:        "http-json",
		SessionMode:      mode,
		SupportsActions:  []string{"navigate", "click", "js_click", "fill", "submit", "press_key", "evaluate"},
		SupportsCapture:  true,
		SupportsSessions: true,
	}
}

func countAomNodes(nodes []*browserpkg.PrunedNode) int {
	total := 0
	var walk func([]*browserpkg.PrunedNode)
	walk = func(items []*browserpkg.PrunedNode) {
		for _, item := range items {
			total++
			walk(item.Children)
		}
	}
	walk(nodes)
	return total
}

func normalizeWait(waitMs int, fallback int) int {
	if waitMs > 0 {
		return waitMs
	}
	return fallback
}

func truncateWithWarning(field string, value string, maxLen int, warnings *[]string) string {
	if maxLen <= 0 || len(value) <= maxLen {
		return value
	}
	if warnings != nil {
		*warnings = append(*warnings, fmt.Sprintf("%s truncated to %d bytes", field, maxLen))
	}
	return value[:maxLen]
}

func validateAbsoluteURL(raw string) error {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return fmt.Errorf("url is required")
	}
	parsed, err := url.ParseRequestURI(trimmed)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return fmt.Errorf("url must be a valid absolute URL")
	}
	return nil
}

func readRuntimeCookies(session *browserpkg.Session) ([]RuntimeCaptureCookie, error) {
	var rawCookies []*network.Cookie
	if err := chromedp.Run(session.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		var err error
		rawCookies, err = network.GetCookies().Do(ctx)
		return err
	})); err != nil {
		return nil, err
	}
	cookies := make([]RuntimeCaptureCookie, 0, len(rawCookies))
	for _, cookie := range rawCookies {
		cookies = append(cookies, RuntimeCaptureCookie{Name: cookie.Name, Value: cookie.Value})
	}
	return cookies, nil
}

func readRuntimeStorage(session *browserpkg.Session) (map[string]string, map[string]string, error) {
	localStorage := map[string]string{}
	sessionStorage := map[string]string{}
	if err := chromedp.Run(
		session.Ctx,
		chromedp.Evaluate(`Object.fromEntries(Object.entries(window.localStorage || {}).map(([key, value]) => [key, String(value)]))`, &localStorage),
		chromedp.Evaluate(`Object.fromEntries(Object.entries(window.sessionStorage || {}).map(([key, value]) => [key, String(value)]))`, &sessionStorage),
	); err != nil {
		return nil, nil, err
	}
	return localStorage, sessionStorage, nil
}

func buildFlowScript(req GenerateRequest) (string, []string, error) {
	startURL := strings.TrimSpace(req.StartURL)
	targetLabel := strings.TrimSpace(req.TargetLabel)
	if startURL == "" {
		return "", nil, fmt.Errorf("startUrl is required")
	}
	if _, err := url.ParseRequestURI(startURL); err != nil {
		return "", nil, fmt.Errorf("startUrl must be a valid absolute URL: %w", err)
	}
	if targetLabel == "" {
		return "", nil, fmt.Errorf("targetLabel is required")
	}

	warnings := make([]string, 0, 1)
	if len(targetLabel) < 3 {
		warnings = append(warnings, "Very short targetLabel values may match multiple elements.")
	}

	script := fmt.Sprintf(`const flow = {
  startUrl: %q,
  strategy: "label-driven",
  steps: [
    { action: "navigate", url: %q },
    { action: "waitForText", text: %q, timeoutMs: 10000 },
    { action: "clickText", text: %q },
    { action: "assertVisible", text: %q }
  ]
};

export default flow;
`, startURL, startURL, targetLabel, targetLabel, targetLabel)

	return script, warnings, nil
}

func handleRuntimeVisualArtifact(c *gin.Context, newRuntimeBrowser runtimeBrowserFactory) {
	var req RuntimeVisualArtifactRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	requestURL := strings.TrimSpace(req.URL)
	if requestURL == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "url is required"})
		return
	}
	if _, err := url.ParseRequestURI(requestURL); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": fmt.Sprintf("url must be a valid absolute URL: %v", err)})
		return
	}

	browserSession, err := newRuntimeBrowser()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": fmt.Sprintf("failed to start runtime browser: %v", err)})
		return
	}
	defer browserSession.Close()

	if err := browserSession.Navigate(requestURL); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": fmt.Sprintf("runtime navigate failed: %v", err)})
		return
	}
	if err := browserSession.WaitForStability(5 * time.Second); err != nil {
		log.Printf("runtime visual artifact stability wait warning: %v", err)
	}

	pngBytes, err := browserSession.CaptureScreenshot()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": fmt.Sprintf("runtime screenshot failed: %v", err)})
		return
	}
	capturedURL, err := browserSession.CurrentURL()
	if err != nil || strings.TrimSpace(capturedURL) == "" {
		capturedURL = requestURL
	}

	c.Header("Content-Type", "image/png")
	c.Header("Cache-Control", "no-store")
	c.Header("X-Runtime-Artifact-Kind", "runtime_screenshot")
	c.Header("X-Runtime-Page-Url", capturedURL)
	c.Data(http.StatusOK, "image/png", pngBytes)
}
