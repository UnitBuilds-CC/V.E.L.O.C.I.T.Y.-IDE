package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/signal"
	"math/rand"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"


	cdpTarget "github.com/chromedp/cdproto/target"
	"github.com/chromedp/chromedp"
	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

type JSONRPCRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      interface{}     `json:"id"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params"`
}

type JSONRPCResponse struct {
	JSONRPC string      `json:"jsonrpc"`
	ID      interface{} `json:"id,omitempty"`
	Result  interface{} `json:"result,omitempty"`
	Error   interface{} `json:"error,omitempty"`
}

type ToolCallParams struct {
	Name      string                 `json:"name"`
	Arguments map[string]interface{} `json:"arguments"`
	Meta      map[string]interface{} `json:"_meta"`
}

type DatasetEntry struct {
	Timestamp            string           `json:"timestamp"`
	CaptchaType          string           `json:"captcha_type"`
	Procedure            string           `json:"procedure"`
	Prompt               string           `json:"prompt,omitempty"`
	ChallengeScreenshot  string           `json:"challenge_screenshot"`
	VerificationScreenshot string         `json:"verification_screenshot"`
	Actions              []browser.Action `json:"actions"`
	Status               string           `json:"status"`
	AOM                  string           `json:"aom,omitempty"`
}

var lastChallengeScreenshot string
var instanceDir string

var sess *browser.Session
var sessMu sync.Mutex

func main() {
	if len(os.Args) > 1 {
		for _, arg := range os.Args[1:] {
			if arg == "-h" || arg == "--help" || arg == "-help" {
				fmt.Println("Agentic Browser MCP Server (Go Edition)")
				fmt.Println("Usage: agentic-mcp [flags]")
				fmt.Println("This server communicates using JSON-RPC over Stdin/Stdout.")
				return
			}
		}
	}

	if os.Getenv("GOOS") == "windows" || os.PathSeparator == '\\' {
		instanceDir = fmt.Sprintf("c:\\go-engine\\instances\\instance_%d", time.Now().Unix())
	} else {
		instanceDir = fmt.Sprintf("%s/instance_%d", os.TempDir(), time.Now().Unix())
	}
	os.MkdirAll(instanceDir, 0755)
	
	// Handle OS signals for graceful shutdown
	c := make(chan os.Signal, 1)
	signal.Notify(c, os.Interrupt, syscall.SIGTERM)
	go func() {
		<-c
		sessMu.Lock()
		if sess != nil {
			sess.Close()
		}
		sessMu.Unlock()
		os.Exit(0)
	}()

	scanner := bufio.NewScanner(os.Stdin)
	// Increase max token size to 10MB to handle large base64/AOM tool calls
	buf := make([]byte, 0, 64*1024)
	scanner.Buffer(buf, 10*1024*1024)

	for scanner.Scan() {
		line := scanner.Bytes()
		appendLog(fmt.Sprintf("RECV_NEW: %s", string(line)))
		var req JSONRPCRequest
		if err := json.Unmarshal(line, &req); err != nil {
			appendLog(fmt.Sprintf("ERR: Unmarshal failed: %v", err))
			continue
		}
		handleRequest(req)
	}

	// Clean up browser session on EOF (scanner finished)
	sessMu.Lock()
	if sess != nil {
		appendLog("EOF: Closing active browser session...")
		sess.Close()
		sess = nil
	}
	sessMu.Unlock()
}

func appendLog(msg string) {
	const maxLogLength = 1000
	if len(msg) > maxLogLength {
		msg = msg[:maxLogLength] + fmt.Sprintf("... [truncated %d bytes]", len(msg)-maxLogLength)
	}
	f, err := os.OpenFile("C:\\go-engine\\mcp_debug.log", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err == nil {
		fmt.Fprintf(f, "[%s] %s\n", time.Now().Format("15:04:05"), msg)
		f.Close()
	}
}

func handleRequest(req JSONRPCRequest) {
	if req.ID == nil {
		return
	}

	resp := JSONRPCResponse{
		JSONRPC: "2.0",
		ID:      req.ID,
	}

	switch req.Method {
	case "initialize":
		exe, _ := os.Executable()
		cwd, _ := os.Getwd()
		appendLog(fmt.Sprintf("INIT: exe=%s, cwd=%s", exe, cwd))
		resp.Result = map[string]interface{}{
			"protocolVersion": "2024-11-05",
			"capabilities": map[string]interface{}{
				"tools": map[string]interface{}{},
			},
			"serverInfo": map[string]string{
				"name":    "agentic-browser-go-v3",
				"version": "1.0.0",
			},
		}

	case "notifications/initialized":
		return

	case "tools/list":
		resp.Result = map[string]interface{}{
			"tools": []map[string]interface{}{
				{
					"name":        "navigate",
					"description": "Navigate the browser to a URL and wait for it to load. Returns the AOM tree of the loaded page. The browser is headful and hardened against bot detection.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"url":         map[string]string{"type": "string"},
							"withSpatial": map[string]string{"type": "boolean"},
							"withStyles":  map[string]string{"type": "boolean"},
						},
						"required": []string{"url"},
					},
				},
				{
					"name":        "fast_navigate",
					"description": "Navigate without returning AOM. Useful for debugging.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"url": map[string]string{"type": "string"},
						},
						"required": []string{"url"},
					},
				},
				{
					"name":        "get_aom",
					"description": "Get the current page's Accessibility Object Model (AOM) tree. If 'withSpatial' is true, nodes include absolute [x, y, w, h] coordinates relative to the top-level viewport. Use these coordinates for click_xy and drag_xy.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"withSpatial": map[string]string{"type": "boolean"},
							"withStyles":  map[string]string{"type": "boolean"},
						},
					},
				},
				{
					"name":        "click",
					"description": "Click an element by its ref ID from the AOM tree. This performs a physical mouse click at the element's center, automatically translating coordinates from the viewport to hit the target correctly, even inside iframes.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"nodeId":      map[string]string{"type": "string"},
							"withSpatial": map[string]string{"type": "boolean"},
							"withStyles":  map[string]string{"type": "boolean"},
						},
						"required": []string{"nodeId"},
					},
				},
				{
					"name":        "type_text",
					"description": "Type text into an input element.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"nodeId":      map[string]string{"type": "string"},
							"text":        map[string]string{"type": "string"},
							"clear":       map[string]string{"type": "boolean"},
							"withSpatial": map[string]string{"type": "boolean"},
							"withStyles":  map[string]string{"type": "boolean"},
						},
						"required": []string{"text"},
					},
				},
				{
					"name":        "natural_type",
					"description": "Type text into an input element with human-like delays.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"nodeId":      map[string]string{"type": "string"},
							"text":        map[string]string{"type": "string"},
							"clear":       map[string]string{"type": "boolean"},
							"withSpatial": map[string]string{"type": "boolean"},
							"withStyles":  map[string]string{"type": "boolean"},
						},
						"required": []string{"text"},
					},
				},
				{
					"name":        "wait_for_load",
					"description": "Wait for the page to finish loading.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"withSpatial": map[string]string{"type": "boolean"},
						},
					},
				},
				{
					"name":        "solve_datadome",
					"description": "Detect and solve DataDome CAPTCHA automatically if present.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{},
					},
				},
				{
					"name":        "scroll",
					"description": "Scroll the page up or down. Direction must be 'up' or 'down'. Amount is in pixels (default 300).",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"direction":   map[string]string{"type": "string"},
							"amount":      map[string]string{"type": "number"},
							"withSpatial": map[string]string{"type": "boolean"},
						},
						"required": []string{"direction"},
					},
				},
				{
					"name":        "wait",
					"description": "Wait for a specified number of milliseconds.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"ms":          map[string]string{"type": "number"},
							"withSpatial": map[string]string{"type": "boolean"},
						},
						"required": []string{"ms"},
					},
				},
				{
					"name":        "press_key",
					"description": "Press a keyboard key (e.g. 'Enter', 'Tab', 'Escape').",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"key":         map[string]string{"type": "string"},
							"withSpatial": map[string]string{"type": "boolean"},
						},
						"required": []string{"key"},
					},
				},
				{
					"name":        "take_screenshot",
					"description": "Take a screenshot of the current browser viewport. Returns the local path to the saved PNG image.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{},
					},
				},
				{
					"name":        "take_node_screenshot",
					"description": "Take a screenshot of a specific element (node) by its ref ID. Returns the local path to the cropped PNG image.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"nodeId": map[string]string{"type": "string"},
						},
						"required": []string{"nodeId"},
					},
				},
				{
					"name":        "inspect_node",
					"description": "Retrieve deep metadata (exact styles, coordinates, and attributes) for a single node by its ref ID.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"nodeId": map[string]string{"type": "string"},
						},
						"required": []string{"nodeId"},
					},
				},
				{
					"name":        "list_frames",
					"description": "List all frames (including iFrames) in the current page.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{},
					},
				},
				{
					"name":        "switch_to_frame",
					"description": "Switch the active browser context to a specific frame (iFrame) by its target ID. This is required to access AOM nodes or take screenshots of content inside an iFrame. Note: Physical actions (click_xy, drag_xy) always use top-level coordinates and do NOT need switching.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"targetId": map[string]string{"type": "string"},
						},
						"required": []string{"targetId"},
					},
				},
				{
					"name":        "switch_to_main_frame",
					"description": "Switch the active browser context back to the main page.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{},
					},
				},
				{
					"name":        "click_xy",
					"description": "Perform a physical click at logical coordinates. If 'nodeId' is provided, X and Y are relative to that element's top-left corner. Otherwise, they are absolute viewport coordinates. Use this to bypass complex UI or captchas.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"x":      map[string]interface{}{"type": "number"},
							"y":      map[string]interface{}{"type": "number"},
							"nodeId": map[string]interface{}{"type": "string", "description": "Optional: Element to use as coordinate origin"},
						},
						"required": []string{"x", "y"},
					},
				},
				{
					"name":        "drag_xy",
					"description": "Perform a humanoid drag-and-drop from (X1, Y1) to (X2, Y2). If 'nodeId' is provided, all coordinates are relative to that element's top-left corner. Optimized for bypassing slider-based bot challenges like DataDome.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"x1":     map[string]interface{}{"type": "number"},
							"y1":     map[string]interface{}{"type": "number"},
							"x2":     map[string]interface{}{"type": "number"},
							"y2":     map[string]interface{}{"type": "number"},
							"nodeId": map[string]interface{}{"type": "string", "description": "Optional: Element to use as coordinate origin"},
						},
						"required": []string{"x1", "y1", "x2", "y2"},
					},
				},
				{
					"name":        "query_selector",
					"description": "Find elements by CSS selector. Returns a JSON array of matches with {tag, id, class, title, x, y, w, h} bounding boxes. Use this to discover elements invisible to the AOM (e.g. custom divs, canvas, color pickers). The returned x/y/w/h are viewport coordinates — feed them directly to click_xy or drag_xy. Capped at 50 results.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"selector": map[string]interface{}{"type": "string", "description": "CSS selector (e.g. 'canvas', '.my-class', '#my-id')"},
						},
						"required": []string{"selector"},
					},
				},
				{
					"name":        "query_area",
					"description": "Find elements within a specific screen area. Returns a JSON array of matches with bounding boxes and metadata. Capped at 50 results.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"x1": map[string]interface{}{"type": "number"},
							"y1": map[string]interface{}{"type": "number"},
							"x2": map[string]interface{}{"type": "number"},
							"y2": map[string]interface{}{"type": "number"},
						},
						"required": []string{"x1", "y1", "x2", "y2"},
					},
				},
				{
					"name":        "execute_batch",
					"description": "Execute a sequence of high-precision humanoid actions (click, drag, type, wait) as an atomic task. Ideal for dynamic challenges.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"actions": map[string]interface{}{
								"type": "array",
								"items": map[string]interface{}{
									"type": "object",
									"properties": map[string]interface{}{
										"type":   map[string]interface{}{"type": "string", "description": "click, drag, type, wait, press"},
										"x":      map[string]interface{}{"type": "number"},
										"y":      map[string]interface{}{"type": "number"},
										"x2":     map[string]interface{}{"type": "number"},
										"y2":     map[string]interface{}{"type": "number"},
										"text":   map[string]interface{}{"type": "string"},
										"key":    map[string]interface{}{"type": "string"},
										"nodeId": map[string]interface{}{"type": "string"},
										"wait":   map[string]interface{}{"type": "number", "description": "ms"},
									},
									"required": []string{"type"},
								},
							},
						},
						"required": []string{"actions"},
					},
				},
				{
					"name":        "solve_captcha",
					"description": "Solve a CAPTCHA using a specific procedure. Supported: recaptchaTiles(grid=[...]), dataDomeSlider(), turnstile().",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"procedure": map[string]interface{}{
								"type":        "string",
								"description": "Procedure call string. Example: recaptchaTiles([[0,1],[1,0]])",
							},
						},
						"required": []string{"procedure"},
					},
				},
				{
					"name":        "solve_recaptcha_tiles",
					"description": "Legacy alias for reCAPTCHA tiles solver.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"grid": map[string]interface{}{
								"type":  "array",
								"items": map[string]interface{}{"type": "object"},
							},
						},
					},
				},
				{
					"name":        "execute_sequence",
					"description": "Execute a sequence of high-precision humanoid actions and optionally save them as a named command.",
					"inputSchema": map[string]interface{}{
						"type": "object",
						"properties": map[string]interface{}{
							"actions": map[string]interface{}{
								"type": "array",
								"items": map[string]interface{}{
									"type": "object",
									"properties": map[string]interface{}{
										"type":   map[string]interface{}{"type": "string", "description": "click, drag, type, wait, press"},
										"x":      map[string]interface{}{"type": "number"},
										"y":      map[string]interface{}{"type": "number"},
										"x2":     map[string]interface{}{"type": "number"},
										"y2":     map[string]interface{}{"type": "number"},
										"text":   map[string]interface{}{"type": "string"},
										"key":    map[string]interface{}{"type": "string"},
										"nodeId": map[string]interface{}{"type": "string"},
										"wait":   map[string]interface{}{"type": "number", "description": "ms"},
									},
									"required": []string{"type"},
								},
							},
							"params": map[string]interface{}{
								"type":        "object",
								"description": "Parameters to replace placeholders in the sequence (e.g., {'to': 'user@example.com'}).",
							},
							"save": map[string]interface{}{
								"type":        "boolean",
								"description": "Save this sequence as a repeatable command.",
							},
							"name": map[string]interface{}{
								"type":        "string",
								"description": "The name of the saved command (e.g., 'gmail - send email').",
							},
						},
						"required": []string{"actions"},
					},
				},
			},
		}

	case "tools/call":
		var params ToolCallParams
		if err := json.Unmarshal(req.Params, &params); err != nil {
			appendLog(fmt.Sprintf("ERR: Unmarshal params failed: %v", err))
			fmt.Fprintf(os.Stderr, "      [MCP Server] Error unmarshaling params: %v\n", err)
			return
		}
		
		type toolResult struct {
			result interface{}
		}
		resChan := make(chan toolResult, 1)
		
		go func() {
			defer func() {
				if r := recover(); r != nil {
					appendLog(fmt.Sprintf("PANIC in tool call goroutine %s: %v", params.Name, r))
					fmt.Fprintf(os.Stderr, "      [MCP Server] PANIC in tool call goroutine %s: %v\n", params.Name, r)
					resChan <- toolResult{
						result: map[string]interface{}{
							"isError": true,
							"content": []map[string]string{
								{"type": "text", "text": fmt.Sprintf("Internal error (panic): %v", r)},
							},
						},
					}
				}
			}()
			resChan <- toolResult{result: handleToolCall(params)}
		}()
		
		select {
		case res := <-resChan:
			resp.Result = res.result
		case <-time.After(15 * time.Second):
			appendLog(fmt.Sprintf("TIMEOUT: Tool call %s timed out after 15s", params.Name))
			fmt.Fprintf(os.Stderr, "      [MCP Server] Tool call %s timed out after 15s\n", params.Name)
			
			sessMu.Lock()
			if sess != nil {
				appendLog("TIMEOUT: Closing browser session...")
				sess.Close()
				sess = nil
			}
			sessMu.Unlock()
			
			resp.Error = map[string]interface{}{
				"code":    -32603,
				"message": fmt.Sprintf("Tool call %s timed out after 15 seconds", params.Name),
			}
		}

	case "ping":
		resp.Result = map[string]interface{}{}

	default:
		fmt.Fprintf(os.Stderr, "      [MCP Server] Unknown method: %s\n", req.Method)
		if req.ID != nil {
			resp.Error = map[string]interface{}{
				"code":    -32601,
				"message": "Method not found",
			}
		} else {
			return // Don't respond to unknown notifications
		}
	}

	sendResponse(resp)
}

func respondWithAom(sess *browser.Session, msg string, cfg browser.AomConfig) interface{} {
	aom, err := sess.GetSummarizedAomFast()
	if err != nil {
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": msg + " (Summary unavailable: " + err.Error() + ")"}}}
	}
	
	// Trigger full AOM fetch and Neo4J save in background (Disabled because it blocks the browser session on heavy sites!)
	// go func() {
	// 	cfg := browser.AomConfig{MaxLength: 95000}
	// 	sess.GetAom(cfg)
	// }()
	
	return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": msg + "\n\nSummary:\n" + aom}}}
}

func handleToolCall(params ToolCallParams) (result interface{}) {
	start := time.Now()
	appendLog(fmt.Sprintf("EXEC START: %s", params.Name))
	defer func() {
		appendLog(fmt.Sprintf("EXEC END: %s (took %v)", params.Name, time.Since(start)))
	}()

	// Panic recovery — bare type assertions on malformed input would crash the server
	defer func() {
		if r := recover(); r != nil {
			fmt.Fprintf(os.Stderr, "      [MCP Server] PANIC in tool %s: %v\n", params.Name, r)
			result = map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Internal error: %v", r)}}}
		}
	}()

	var s *browser.Session
	sessMu.Lock()
	if sess == nil || !sess.IsAlive() {
		if sess != nil {
			fmt.Fprintf(os.Stderr, "      [MCP Server] Session lost, restarting browser...\n")
		}
		newSess, err := browser.NewManagedSession()
		if err != nil {
			sessMu.Unlock()
			msg := fmt.Sprintf("Error starting browser: %v", err)
			appendLog("ERR: " + msg)
			fmt.Fprintf(os.Stderr, "      [MCP Server] %s\n", msg)
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": msg}}}
		}
		sess = newSess
	}
	s = sess
	sessMu.Unlock()

	switch params.Name {
	case "navigate":
		url := params.Arguments["url"].(string)
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		
		fmt.Fprintf(os.Stderr, "      [MCP Server] Navigating to: %s (spatial: %v, styles: %v)\n", url, spatial, styles)
		if err := s.Navigate(url); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Navigation failed: %v", err)}}}
		}
		
		msg := "Navigated to " + url
		return respondWithAom(s, msg, cfg)

	case "fast_navigate":
		url := params.Arguments["url"].(string)
		
		fmt.Fprintf(os.Stderr, "      [MCP Server] Fast Navigating to: %s\n", url)
		if err := s.Navigate(url); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Navigation failed: %v", err)}}}
		}
		
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": "Navigated to " + url}}}
	
	case "get_aom":
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		
		aom, err := s.GetAom(cfg)
		if err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Failed to get AOM: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": aom}}}

	case "click":
		nodeId := params.Arguments["nodeId"].(string)
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		
		if err := s.JSClick(nodeId); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Click failed: %v", err)}}}
		}
		s.WaitForStability(0)
		return respondWithAom(s, "Clicked "+nodeId, cfg)

	case "type_text":
		nodeId := ""
		if val, ok := params.Arguments["nodeId"].(string); ok {
			nodeId = val
		}
		text := params.Arguments["text"].(string)
		clear, _ := params.Arguments["clear"].(bool)
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		
		targetNodeId := nodeId
		if clear {
			targetNodeId = ""
			if nodeId != "" {
				s.Click(nodeId)
				time.Sleep(time.Duration(200+rand.Intn(200)) * time.Millisecond)
			}
			s.PressKey("Ctrl+A")
			time.Sleep(time.Duration(300+rand.Intn(300)) * time.Millisecond)
			s.PressKey("Backspace")
			time.Sleep(time.Duration(200+rand.Intn(200)) * time.Millisecond)
		}
		
		if err := s.TypeText(targetNodeId, text); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Typing failed: %v", err)}}}
		}
		return respondWithAom(s, "Typed into "+nodeId, cfg)

	case "natural_type":
		nodeId := ""
		if val, ok := params.Arguments["nodeId"].(string); ok {
			nodeId = val
		}
		text := params.Arguments["text"].(string)
		clear, _ := params.Arguments["clear"].(bool)
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		
		targetNodeId := nodeId
		if clear {
			targetNodeId = ""
			if nodeId != "" {
				s.Click(nodeId)
				time.Sleep(time.Duration(200+rand.Intn(200)) * time.Millisecond)
			}
			s.PressKey("Ctrl+A")
			time.Sleep(time.Duration(300+rand.Intn(300)) * time.Millisecond)
			s.PressKey("Backspace")
			time.Sleep(time.Duration(200+rand.Intn(200)) * time.Millisecond)
		}
		
		if err := s.TypeNatural(targetNodeId, text); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Typing failed: %v", err)}}}
		}
		return respondWithAom(s, "Naturally typed into "+nodeId, cfg)

	case "wait_for_load":
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		s.WaitForStability(0)
		return respondWithAom(s, "Wait complete", cfg)

	case "scroll":
		direction := params.Arguments["direction"].(string)
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		amount := 300
		if val, ok := params.Arguments["amount"].(float64); ok {
			amount = int(val)
		}
		s.Scroll(direction, amount)
		return respondWithAom(s, "Scrolled "+direction, cfg)

	case "wait":
		ms := 1000
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		if val, ok := params.Arguments["ms"].(float64); ok {
			ms = int(val)
		}
		time.Sleep(time.Duration(ms) * time.Millisecond)
		return respondWithAom(s, fmt.Sprintf("Waited %dms", ms), cfg)

	case "press_key":
		key := params.Arguments["key"].(string)
		spatial, _ := params.Arguments["withSpatial"].(bool)
		styles, _ := params.Arguments["withStyles"].(bool)
		cfg := browser.AomConfig{WithSpatial: spatial, WithStyles: styles, MaxLength: 30000}
		s.PressKey(key)
		return respondWithAom(s, "Pressed "+key, cfg)

	case "take_screenshot":
		path := fmt.Sprintf("%s/screenshot_%d.png", instanceDir, time.Now().Unix())
		if artifactsDir, ok := params.Meta["antigravity.google/artifacts_dir"].(string); ok {
			path = fmt.Sprintf("%s/screenshot_%d.png", artifactsDir, time.Now().Unix())
		}
		
		if err := s.TakeScreenshot(path); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Screenshot failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": "Screenshot saved to " + path}}}

	case "take_node_screenshot":
		nodeId := params.Arguments["nodeId"].(string)
		path := fmt.Sprintf("%s/agentic-browser-node-%s-%d.png", os.TempDir(), nodeId, time.Now().Unix())
		if artifactsDir, ok := params.Meta["antigravity.google/artifacts_dir"].(string); ok {
			path = fmt.Sprintf("%s/node_%s_%d.png", artifactsDir, nodeId, time.Now().Unix())
		}

		if err := s.TakeNodeScreenshot(nodeId, path); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Node screenshot failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": "Node screenshot saved to " + path}}}

	case "list_frames":
		frames, err := s.GetFrames()
		if err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("List frames failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": frames}}}

	case "switch_to_frame":
		targetId := params.Arguments["targetId"].(string)
		if err := s.SwitchToFrame(targetId); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Switch to frame failed: %v", err)}}}
		}
		return respondWithAom(s, "Switched to frame "+targetId, browser.AomConfig{MaxLength: 30000})

	case "switch_to_main_frame":
		if err := s.SwitchToMainFrame(); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Switch to main frame failed: %v", err)}}}
		}
		return respondWithAom(s, "Switched to main frame", browser.AomConfig{MaxLength: 30000})

	case "click_xy":
		x := params.Arguments["x"].(float64)
		y := params.Arguments["y"].(float64)
		nodeID, _ := params.Arguments["nodeId"].(string)
		fmt.Fprintf(os.Stderr, "      [MCP Server] Clicking at: (%f, %f) [Node: %s]\n", x, y, nodeID)
		if err := s.ClickXY(x, y, nodeID); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("ClickXY failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Clicked at (%f, %f) [Node: %s]", x, y, nodeID)}}}

	case "drag_xy":
		x1 := params.Arguments["x1"].(float64)
		y1 := params.Arguments["y1"].(float64)
		x2 := params.Arguments["x2"].(float64)
		y2 := params.Arguments["y2"].(float64)
		nodeID, _ := params.Arguments["nodeId"].(string)
		fmt.Fprintf(os.Stderr, "      [MCP Server] Dragging from (%f, %f) to (%f, %f) [Node: %s]\n", x1, y1, x2, y2, nodeID)
		if err := s.DragXY(x1, y1, x2, y2, nodeID); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("DragXY failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Dragged from (%f, %f) to (%f, %f) [Node: %s]", x1, y1, x2, y2, nodeID)}}}

	case "inspect_node":
		nodeIdStr := params.Arguments["nodeId"].(string)
		var nodeId int64
		fmt.Sscanf(nodeIdStr, "%d", &nodeId)
		
		node, err := s.InspectNode(nodeId)
		if err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Inspection failed: %v", err)}}}
		}
		
		res, _ := json.MarshalIndent(node, "", "  ")
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": string(res)}}}


	case "query_selector":
		selector := params.Arguments["selector"].(string)
		result, err := s.QuerySelector(selector)
		if err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("QuerySelector failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": result}}}

	case "query_area":
		x1 := params.Arguments["x1"].(float64)
		y1 := params.Arguments["y1"].(float64)
		x2 := params.Arguments["x2"].(float64)
		y2 := params.Arguments["y2"].(float64)
		result, err := s.QueryArea(x1, y1, x2, y2)
		if err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("QueryArea failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": result}}}

	case "solve_captcha":
		proc := params.Arguments["procedure"].(string)
		res := handleSolveCaptcha(s, proc)
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": res}}}

	case "solve_recaptcha_tiles":
		grid := params.Arguments["grid"]
		gridData, _ := json.Marshal(grid)
		res := handleSolveCaptcha(s, fmt.Sprintf("recaptchaTiles(%s)", string(gridData)))
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": res}}}

	case "execute_batch":
		var actions []browser.Action
		actionsData, _ := json.Marshal(params.Arguments["actions"])
		json.Unmarshal(actionsData, &actions)
		
		if err := s.ExecuteBatch(actions); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("ExecuteBatch failed: %v", err)}}}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": "Batch executed successfully"}}}

	case "execute_sequence":
		var actions []browser.Action
		name, _ := params.Arguments["name"].(string)
		save, _ := params.Arguments["save"].(bool)
		execParams, _ := params.Arguments["params"].(map[string]interface{})
		
		// If actions are provided, use them. Otherwise, try to load by name.
		if params.Arguments["actions"] != nil {
			actionsData, _ := json.Marshal(params.Arguments["actions"])
			json.Unmarshal(actionsData, &actions)
		} else if name != "" {
			if s.DbClient == nil {
				return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": "Neo4j client not initialized"}}}
			}
			actionsJSON, err := s.DbClient.GetSequence(context.Background(), name)
			if err != nil {
				return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("Failed to load sequence %s: %v", name, err)}}}
			}
			json.Unmarshal([]byte(actionsJSON), &actions)
		}
		
		if len(actions) == 0 {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": "No actions to execute"}}}
		}
		
		// Replace placeholders in actions
		if execParams != nil {
			for i := range actions {
				for k, v := range execParams {
					valStr := fmt.Sprintf("%v", v)
					placeholder := fmt.Sprintf("{{%s}}", k)
					actions[i].Text = strings.ReplaceAll(actions[i].Text, placeholder, valStr)
				}
			}
		}
		
		if err := s.ExecuteBatch(actions); err != nil {
			return map[string]interface{}{"isError": true, "content": []map[string]string{{"type": "text", "text": fmt.Sprintf("ExecuteBatch failed: %v", err)}}}
		}
		
		msg := "Sequence executed successfully"
		if save && name != "" {
			if s.DbClient == nil {
				msg += ", but failed to save: Neo4j client not initialized"
			} else {
				actionsData, _ := json.Marshal(params.Arguments["actions"])
				err := s.DbClient.SaveSequence(context.Background(), name, string(actionsData))
				if err != nil {
					msg += fmt.Sprintf(", but failed to save: %v", err)
				} else {
					msg += fmt.Sprintf(" and saved to Neo4j as %s", name)
				}
			}
		}
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": msg}}}



	default:
		return map[string]interface{}{"content": []map[string]string{{"type": "text", "text": "Tool not found: " + params.Name}}}
	}
}

func autoSolveDataDome(sess *browser.Session) string {
	appendLog("      [DataDome] Auto-solve sequence initiated")
	
	// 1. Find the DataDome iframe target ID and its absolute position
	var targetID string
	var iframeX, iframeY float64

	if err := chromedp.Run(sess.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		infos, err := cdpTarget.GetTargets().Do(ctx)
		if err != nil { return err }
		for _, info := range infos {
			if strings.Contains(info.URL, "geo.captcha-delivery.com") || strings.Contains(info.Title, "DataDome") {
				targetID = string(info.TargetID)
				break
			}
		}
		return nil
	})); err != nil {
		appendLog(fmt.Sprintf("      [DataDome] Target lookup failed: %v", err))
		return ""
	}
	if targetID == "" {
		appendLog("      [DataDome] No challenge iframe target found")
		return "No DataDome challenge iframe found."
	}

	// 2. Get iframe position in main frame
	jsonStr, _ := sess.QuerySelector("iframe")
	var allIframes []map[string]interface{}
	json.Unmarshal([]byte(jsonStr), &allIframes)
	
	for _, f := range allIframes {
		title := fmt.Sprintf("%v", f["title"])
		class := fmt.Sprintf("%v", f["class"])
		if strings.Contains(title, "DataDome") || strings.Contains(class, "captcha") {
			fw := f["w"].(float64)
			fh := f["h"].(float64)
			if fw > 100 && fh > 100 {
				iframeX = f["x"].(float64)
				iframeY = f["y"].(float64)
				appendLog(fmt.Sprintf("      [DataDome] Active iframe found at (%f, %f) size %fx%f", iframeX, iframeY, fw, fh))
				break
			}
		}
	}

	if iframeX == 0 && iframeY == 0 {
		appendLog("      [DataDome] Warning: Iframe at 0,0 or not found by specific attributes. Falling back to first iframe.")
		if len(allIframes) > 0 {
			iframeX = allIframes[0]["x"].(float64)
			iframeY = allIframes[0]["y"].(float64)
		}
	}
	fmt.Fprintf(os.Stderr, "      [MCP Server] Switching to DataDome frame: %s\n", targetID)
	if err := sess.SwitchToFrame(targetID); err != nil {
		appendLog(fmt.Sprintf("      [DataDome] Frame switch failed: %v", err))
		return fmt.Sprintf("Frame switch failed: %v", err)
	}

	// Detect "RETRY" screen
	jsonStr, _ = sess.QuerySelector("button.retryLink, button#captcha__reload__button, button.retry-button, .retry-button, button")
	var nodes []map[string]interface{}
	json.Unmarshal([]byte(jsonStr), &nodes)
	
	retryFound := false
	for _, n := range nodes {
		if strings.Contains(strings.ToLower(fmt.Sprintf("%v", n["id"])), "retry") || 
		   strings.Contains(strings.ToLower(fmt.Sprintf("%v", n["class"])), "retry") {
			appendLog("      [DataDome] Retry screen detected. Clicking...")
			rx := n["x"].(float64) + n["w"].(float64)/2
			ry := n["y"].(float64) + n["h"].(float64)/2
			sess.ClickXY(rx, ry, "")
			retryFound = true
			break
		}
	}

	if retryFound {
		time.Sleep(3 * time.Second)
	}

	// Detect slider track inside iframe
	jsonStr, _ = sess.QuerySelector("#captcha__element, .captcha__element, div.captcha__element, #slider-track, .slider-track")
	json.Unmarshal([]byte(jsonStr), &nodes)
	
	// Detect slider and target specifically
	jsonStr, _ = sess.QuerySelector(".slider, .sliderTarget")
	json.Unmarshal([]byte(jsonStr), &nodes)
	
	var slider, target map[string]interface{}
	for _, n := range nodes {
		if strings.Contains(fmt.Sprintf("%v", n["class"]), "sliderTarget") {
			target = n
		} else if strings.Contains(fmt.Sprintf("%v", n["class"]), "slider") {
			slider = n
		}
	}

	// Switch back to main frame for swiping
	sess.SwitchToMainFrame()

	var x1, x2, y float64
	if slider != nil && target != nil {
		appendLog("      [DataDome] Found both slider and target. Calculating centers...")
		x1 = slider["x"].(float64) + slider["w"].(float64)/2 + iframeX
		x2 = target["x"].(float64) + target["w"].(float64)/2 + iframeX
		y = slider["y"].(float64) + slider["h"].(float64)/2 + iframeY
	} else {
		// Try body as fallback
		jsonStr, _ = sess.QuerySelector("body")
		var bodyNodes []map[string]interface{}
		json.Unmarshal([]byte(jsonStr), &bodyNodes)
		
		if len(bodyNodes) > 0 {
			appendLog("      [DataDome] Falling back to generic body swipe")
			bn := bodyNodes[0]
			nx := bn["x"].(float64) + iframeX
			ny := bn["y"].(float64) + iframeY
			nw := bn["w"].(float64)
			nh := bn["h"].(float64)
			x1 = nx + 40
			x2 = nx + nw - 40
			y = ny + nh/2
			if nw > 400 && nh > 300 { y += 50 }
		} else {
			appendLog("      [DataDome] No slider elements found")
			return "No slider elements found."
		}
	}

	appendLog(fmt.Sprintf("      [DataDome] Performing swipe from %f to %f at Y=%f", x1, x2, y))
	fmt.Fprintf(os.Stderr, "      [MCP Server] Auto-swiping from %f to %f at Y=%f\n", x1, x2, y)
	
	if err := sess.DragXY(x1, y, x2, y, ""); err != nil {
		appendLog(fmt.Sprintf("      [DataDome] Swipe failed: %v", err))
		return fmt.Sprintf("Solve failed: %v", err)
	}
	
	appendLog("      [DataDome] Challenge swiped. Waiting for validation...")
	time.Sleep(5 * time.Second) // Wait for redirect
	
	return "DataDome challenge swiped."
}

func autoSolveTurnstile(sess *browser.Session) string {
	appendLog("      [Turnstile] Auto-solve sequence initiated")
	
	// 1. Find the Turnstile iframe target ID
	var targetID string
	appendLog("      [Turnstile] Starting target discovery loop...")
	for i := 0; i < 3; i++ {
		tCtx, tCancel := context.WithTimeout(sess.Ctx, 5*time.Second)
		err := chromedp.Run(tCtx, chromedp.ActionFunc(func(ctx context.Context) error {
			infos, err := cdpTarget.GetTargets().Do(ctx)
			if err != nil { return err }
			appendLog(fmt.Sprintf("      [Turnstile] (Attempt %d) Found %d targets", i+1, len(infos)))
			for _, info := range infos {
				appendLog(fmt.Sprintf("      [Turnstile] Evaluating: %s (%s)", info.Type, info.URL))
				if strings.Contains(info.URL, "challenges.cloudflare.com") && info.Type == "iframe" {
					targetID = string(info.TargetID)
					appendLog(fmt.Sprintf("      [Turnstile] MATCHED TargetID: %s", targetID))
					break
				}
			}
			return nil
		}))
		tCancel()
		if targetID != "" || err != nil { 
			if err != nil { appendLog(fmt.Sprintf("      [Turnstile] Target discovery error: %v", err)) }
			break 
		}
		time.Sleep(1 * time.Second)
	}

	if targetID == "" {
		appendLog("      [Turnstile] No Turnstile target found after retries")
		return "No Turnstile found"
	}

	fmt.Fprintf(os.Stderr, "      [MCP Server] Switching to Turnstile frame: %s\n", targetID)
	sess.SwitchToFrame(targetID)
	defer sess.SwitchToMainFrame()

	time.Sleep(3 * time.Second) // Allow Turnstile to 'verify' and show the checkbox

	// 2. Check for "Success!" or "Verify you are human"
	aom, _ := sess.GetAom(browser.AomConfig{})
	if strings.Contains(aom, "Success!") {
		appendLog("      [Turnstile] Challenge already solved")
		return "Turnstile already solved."
	}

	// 2. Find the checkbox via AOM string parsing (most reliable for Shadow DOM)
	var targetNodeID string
	appendLog("      [Turnstile] Polling AOM for 'Verify you are human' semantic node...")
	
	// Regex to match [role ID] "Name"
	// Example: [checkbox 26] "Verify you are human"
	re := regexp.MustCompile(`\[(checkbox|button) (\d+)\] "Verify you are human"`)
	
	for retry := 0; retry < 15; retry++ {
		aom, _ := sess.GetAom(browser.AomConfig{})
		if strings.Contains(aom, "Success!") {
			appendLog("      [Turnstile] Solved successfully (detected in AOM)")
			return "Turnstile solved successfully."
		}
		
		match := re.FindStringSubmatch(aom)
		if match != nil {
			targetNodeID = match[2]
			appendLog(fmt.Sprintf("      [Turnstile] Found semantic node ID: %s", targetNodeID))
			break
		}
		
		time.Sleep(1 * time.Second)
	}
	if targetNodeID != "" {
		id, _ := strconv.ParseInt(targetNodeID, 10, 64)
		if n, err := sess.InspectNode(id); err == nil {
			relX := float64(n.X + n.W/2)
			relY := float64(n.Y + n.H/2)
			
			// Get iframe global position
			sess.SwitchToMainFrame()
			
			// Find iframe via AOM parsing
			aomMain, _ := sess.GetAom(browser.AomConfig{})
			reIf := regexp.MustCompile(`\[Iframe (\d+)\] "Widget containing a Cloudflare security challenge"`)
			matchIf := reIf.FindStringSubmatch(aomMain)
			
			var iframeNode *browser.PrunedNode
			if matchIf != nil {
				ifID, _ := strconv.ParseInt(matchIf[1], 10, 64)
				iframeNode, _ = sess.InspectNode(ifID)
			}
			
			if iframeNode != nil {
				ifX := float64(iframeNode.X)
				ifY := float64(iframeNode.Y)
				gx := ifX + relX
				gy := ifY + relY
				
				appendLog(fmt.Sprintf("      [Turnstile] Physical Click Global (%f, %f)", gx, gy))
				sess.ClickXY(gx, gy, "")
			} else {
				// Fallback to selectors if AOM failed
				selectors := []string{"iframe[title*='security challenge']", "iframe[src*='challenges.cloudflare.com']"}
				var iframeNodes []map[string]interface{}
				for _, sel := range selectors {
					iframeJson, _ := sess.QuerySelector(sel)
					json.Unmarshal([]byte(iframeJson), &iframeNodes)
					if len(iframeNodes) > 0 { break }
				}
				
				if len(iframeNodes) > 0 {
					ifX := iframeNodes[0]["x"].(float64)
					ifY := iframeNodes[0]["y"].(float64)
					gx := ifX + relX
					gy := ifY + relY
					appendLog(fmt.Sprintf("      [Turnstile] Physical Click Global via Selector (%f, %f)", gx, gy))
					sess.ClickXY(gx, gy, "")
				} else {
					appendLog("      [Turnstile] ERR: Could not find iframe in main context (AOM or Selector)")
					sess.ClickXY(relX, relY, "")
				}
			}
		} else {
			appendLog(fmt.Sprintf("      [Turnstile] Falling back to JS click for node %s", targetNodeID))
			sess.Click(targetNodeID)
		}
		
		time.Sleep(3 * time.Second)
		return "Turnstile interaction attempted."
	}

	return "Turnstile solver: Checkbox not found"
}

func autoSolveRecaptcha(sess *browser.Session) string {
	appendLog("      [reCAPTCHA] Auto-solve sequence initiated")
	
	// 1. Find the reCAPTCHA anchor iframe
	jsonStr, _ := sess.QuerySelector("iframe[title='reCAPTCHA']")
	var iframes []map[string]interface{}
	json.Unmarshal([]byte(jsonStr), &iframes)
	
	if len(iframes) == 0 {
		return "" // No reCAPTCHA found
	}
	
	iframe := iframes[0]
	cx := iframe["x"].(float64) + 30 // Approx checkbox location
	cy := iframe["y"].(float64) + 30
	
	appendLog(fmt.Sprintf("      [reCAPTCHA] Clicking checkbox at (%f, %f)", cx, cy))
	sess.ClickXY(cx, cy, "")
	
	// 2. Wait for challenge iframe
	time.Sleep(2 * time.Second)
	
	jsonStr, _ = sess.QuerySelector("iframe[title*='challenge']")
	var challenges []map[string]interface{}
	json.Unmarshal([]byte(jsonStr), &challenges)
	
	if len(challenges) > 0 {
		cid := fmt.Sprintf("%d", int64(challenges[0]["backendNodeId"].(float64)))
		lastChallengeScreenshot = fmt.Sprintf("C:\\go-engine\\dataset\\images\\challenge_%d.png", time.Now().Unix())
		cwd, _ := os.Getwd()
		appendLog(fmt.Sprintf("      [reCAPTCHA] Challenge detected (Node %s). CWD: %s. Taking screenshot...", cid, cwd))
		if err := sess.TakeNodeScreenshot(cid, lastChallengeScreenshot); err != nil {
			appendLog(fmt.Sprintf("      [reCAPTCHA] Node screenshot failed: %v", err))
			sess.TakeScreenshot(lastChallengeScreenshot) // Fallback to full screenshot
		}
		return "reCAPTCHA Challenge Detected: " + lastChallengeScreenshot
	}
	
	return "reCAPTCHA Checkbox clicked."
}

func autoSolveHcaptcha(sess *browser.Session) string {
	appendLog("      [hCaptcha] Auto-solve sequence initiated")
	
	// 1. Find the hCaptcha anchor iframe via AOM
	aomMain, _ := sess.GetAom(browser.AomConfig{})
	reIf := regexp.MustCompile(`\[Iframe (\d+)\] "Widget containing checkbox for hCaptcha security challenge"`)
	matchIf := reIf.FindStringSubmatch(aomMain)
	
	if matchIf == nil {
		return "" // No hCaptcha found
	}
	
	ifID, _ := strconv.ParseInt(matchIf[1], 10, 64)
	iframeNode, err := sess.InspectNode(ifID)
	if err != nil {
		appendLog(fmt.Sprintf("      [hCaptcha] Inspect failed: %v", err))
		return ""
	}
	
	// hCaptcha checkbox is typically in the center-left of the iframe
	cx := float64(iframeNode.X) + 25
	cy := float64(iframeNode.Y) + float64(iframeNode.H)/2
	
	appendLog(fmt.Sprintf("      [hCaptcha] Clicking checkbox at Global (%f, %f)", cx, cy))
	sess.ClickXY(cx, cy, "")
	
	return "hCaptcha interaction attempted."
}

func autoSolveAllCaptchas(sess *browser.Session) string {
	res := ""
	dd := autoSolveDataDome(sess)
	if !strings.Contains(dd, "No DataDome") { res += dd + " " }
	
	ts := autoSolveTurnstile(sess)
	if ts != "" { res += ts + " " }
	
	hc := autoSolveHcaptcha(sess)
	if hc != "" { res += hc + " " }
	
	rc := autoSolveRecaptcha(sess)
	if rc != "" { res += rc }
	
	if res == "" { return "None detected." }
	return res
}

func sendResponse(resp JSONRPCResponse) {
	data, err := json.Marshal(resp)
	if err != nil {
		fmt.Fprintf(os.Stderr, "      [MCP Server] Error marshaling response: %v\n", err)
		return
	}
	appendLog(fmt.Sprintf("SEND (size %d bytes): %s", len(data), string(data)))
	fmt.Println(string(data))
}
func handleSolveCaptcha(sess *browser.Session, procedure string) string {
	appendLog(fmt.Sprintf("      [Captcha] Handling procedure: %s", procedure))
	
	// Simple parser for procedure(args)
	openParen := strings.Index(procedure, "(")
	closeParen := strings.LastIndex(procedure, ")")
	if openParen == -1 || closeParen == -1 {
		// Try auto-solvers if no parens
		switch procedure {
		case "datadomeSlider":
			return autoSolveDataDome(sess)
		case "turnstile":
			return autoSolveTurnstile(sess)
		}
		return "Invalid procedure format"
	}

	name := procedure[:openParen]
	args := procedure[openParen+1 : closeParen]

	switch name {
	case "recaptchaTiles":
		return solveRecaptchaTiles(sess, args)
	case "datadomeSlider":
		return autoSolveDataDome(sess)
	case "turnstile":
		return autoSolveTurnstile(sess)
	}

	return "Unknown solver: " + name
}

func solveRecaptchaTiles(sess *browser.Session, args string) string {
	// Find challenge iframe location
	jsonStr, _ := sess.QuerySelector("iframe[title*='challenge']")
	var challenges []map[string]interface{}
	json.Unmarshal([]byte(jsonStr), &challenges)
	
	if len(challenges) == 0 {
		return "Challenge iframe not found"
	}
	
	chall := challenges[0]
	iframeX := chall["x"].(float64)
	iframeY := chall["y"].(float64)

	// Parse grid from args
	// Expected format: [[0,1],[1,0]] or array[(0,1),(1,0)]
	var grid [][]int
	
	// Clean up SQL-like array syntax if present
	cleanArgs := strings.ReplaceAll(args, "array[", "[")
	cleanArgs = strings.ReplaceAll(cleanArgs, "(", "[")
	cleanArgs = strings.ReplaceAll(cleanArgs, ")", "]")
	
	if err := json.Unmarshal([]byte(cleanArgs), &grid); err != nil {
		appendLog(fmt.Sprintf("ERR: Failed to parse grid args: %v", err))
		return "Failed to parse grid: " + err.Error()
	}

	if len(grid) == 0 {
		return "Empty grid provided"
	}

	var actions []browser.Action
	rows := len(grid)
	cols := len(grid[0])
	
	tileW := 400.0 / float64(cols)
	tileH := 400.0 / float64(rows)
	
	for r := 0; r < rows; r++ {
		for c := 0; c < cols; c++ {
			if grid[r][c] == 1 {
				cx := iframeX + float64(c)*tileW + tileW/2
				cy := iframeY + 150 + float64(r)*tileH + tileH/2
				actions = append(actions, browser.Action{Type: "click", X: cx, Y: cy})
			}
		}
	}
	
	// Add "Verify" or "Skip" button click
	actions = append(actions, browser.Action{Type: "click", X: iframeX + 330, Y: iframeY + 560})
	
	if err := sess.ExecuteBatch(actions); err != nil {
		return fmt.Sprintf("Batch click failed: %v", err)
	}

	time.Sleep(7 * time.Second)
	
	// Record verification screenshot
	verifyPath := fmt.Sprintf("C:\\go-engine\\dataset\\images\\verify_%d.png", time.Now().Unix())
	if err := sess.TakeScreenshot(verifyPath); err != nil {
		appendLog(fmt.Sprintf("ERR: Failed to take verification screenshot: %v", err))
	} else {
		appendLog(fmt.Sprintf("      [Dataset] Verification screenshot saved: %s", verifyPath))
	}

	// Capture AOM for dataset
	aom, _ := sess.GetAom(browser.AomConfig{WithSpatial: true})

	// Save to dataset
	saveToDataset(DatasetEntry{
		Timestamp:            time.Now().Format(time.RFC3339),
		CaptchaType:          "recaptchaTiles",
		Procedure:            fmt.Sprintf("recaptchaTiles(%s)", args),
		ChallengeScreenshot:  lastChallengeScreenshot,
		VerificationScreenshot: verifyPath,
		Actions:              actions,
		Status:               "success",
		AOM:                  aom,
	})

	return "Recaptcha tiles procedure completed. Trace recorded."
}

func saveToDataset(entry DatasetEntry) {
	appendLog("      [Dataset] Saving interaction trace...")
	f, err := os.OpenFile("C:\\go-engine\\dataset\\traces.jsonl", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		appendLog(fmt.Sprintf("ERR: Failed to open dataset file: %v", err))
		return
	}
	defer f.Close()

	data, _ := json.Marshal(entry)
	f.Write(data)
	f.Write([]byte("\n"))
}
