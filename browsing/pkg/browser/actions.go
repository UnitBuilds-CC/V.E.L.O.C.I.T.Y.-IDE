package browser

import (
	"context"
	"encoding/json"
	"fmt"
	"math/rand"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/chromedp/cdproto/cdp"
	"github.com/chromedp/cdproto/css"
	"github.com/chromedp/cdproto/dom"
	"github.com/chromedp/cdproto/input"
	"github.com/chromedp/cdproto/runtime"
	cdpPage "github.com/chromedp/cdproto/page"
	cdpTarget "github.com/chromedp/cdproto/target"
	"github.com/chromedp/chromedp"
)

// Click attempts a physical click on a node by its spatial coordinates from the AOM.
func (s *Session) Click(nodeID string) error {
	id, err := strconv.ParseInt(nodeID, 10, 64)
	if err != nil {
		// Not a number, treat as CSS selector!
		fmt.Fprintf(os.Stderr, "      [Telemetry] Node ID is not numeric, treating as CSS selector: %s\n", nodeID)
		return chromedp.Run(s.Ctx, chromedp.Click(nodeID))
	}

	// Find node in last AOM to get its spatial coordinates
	var target *PrunedNode
	var find func([]*PrunedNode)
	find = func(ns []*PrunedNode) {
		for _, n := range ns {
			if n.BackendID == id { target = n; return }
			find(n.Children)
		}
	}
	find(s.LastAom)

	if target == nil {
		return fmt.Errorf("node %d not found in last AOM; interaction out of sync", id)
	}

	// 1. Ensure element is in view (human-like)
	if target.IsOffscreen || target.Y < 0 || target.Y > 1000 { // Assuming 1000px viewport for simplicity or get real height
		fmt.Fprintf(os.Stderr, "      [Browser] Element offscreen, scrolling into view: %d\n", id)
		// We use a surgical scroll to center the element
		chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
			obj, err := dom.ResolveNode().WithBackendNodeID(cdp.BackendNodeID(target.BackendID)).Do(ctx)
			if err != nil { return err }
			_, _, err = runtime.CallFunctionOn("function() { this.scrollIntoView({block: 'center', inline: 'center', behavior: 'smooth'}); }").
				WithObjectID(obj.ObjectID).Do(ctx)
			return err
		}))
		time.Sleep(800 * time.Millisecond) // Wait for smooth scroll旋
	}

	// Calculate surgical point in PURE LOGICAL pixels.
	// We target the left-center (15% offset) to hit reCAPTCHA checkboxes directly.
	cX := float64(target.X) + float64(target.W)*0.15
	cY := float64(target.Y) + float64(target.H)/2

	fmt.Fprintf(os.Stderr, "      [Browser] Surgical Logical Click at (%f, %f) on node %d (%s)\n", cX, cY, id, target.Name)

	return chromedp.Run(s.MainCtx, 
		chromedp.MouseEvent("mouseMoved", cX, cY),
		chromedp.MouseClickXY(cX, cY),
	)
}

// ClickXY attempts a physical click at raw logical coordinates.
// ClickXY attempts a physical click at logical coordinates.
// If nodeID is provided, x and y are relative to that node's top-left.
// Otherwise, they are relative to the top-left of the viewport.
func (s *Session) ClickXY(x, y float64, nodeID string) error {
	finalX, finalY := x, y
	if nodeID != "" {
		id, _ := strconv.ParseInt(nodeID, 10, 64)
		if n, err := s.InspectNode(id); err == nil {
			finalX += float64(n.X)
			finalY += float64(n.Y)
		}
	}

	fmt.Fprintf(os.Stderr, "      [Browser] Physical Click at (%f, %f) [Node: %s]\n", finalX, finalY, nodeID)
	return chromedp.Run(s.MainCtx, 
		chromedp.MouseEvent("mouseMoved", finalX, finalY),
		chromedp.MouseClickXY(finalX, finalY),
	)
}

// DragXY moves the mouse from (x1, y1) to (x2, y2) with humanoid jitter.
// If nodeID is provided, all coordinates are relative to that node's top-left.
func (s *Session) DragXY(x1, y1, x2, y2 float64, nodeID string) error {
	offsetX, offsetY := 0.0, 0.0
	if nodeID != "" {
		id, _ := strconv.ParseInt(nodeID, 10, 64)
		if n, err := s.InspectNode(id); err == nil {
			offsetX = float64(n.X)
			offsetY = float64(n.Y)
		}
	}

	fx1, fy1 := x1+offsetX, y1+offsetY
	fx2, fy2 := x2+offsetX, y2+offsetY

	fmt.Fprintf(os.Stderr, "      [Browser] Humanoid Drag from (%f, %f) to (%f, %f) [Node: %s]\n", fx1, fy1, fx2, fy2, nodeID)
	
	// 1. Move to start and press
	if err := chromedp.Run(s.MainCtx, 
		chromedp.MouseEvent("mouseMoved", fx1, fy1),
	); err != nil { return err }
	time.Sleep(100 * time.Millisecond) // Human-like hesitation
	if err := chromedp.Run(s.MainCtx, 
		chromedp.MouseEvent("mousePressed", fx1, fy1, chromedp.Button("left")),
	); err != nil { return err }
	time.Sleep(150 * time.Millisecond)

	// 2. Interpolate path with humanoid jitter and variable velocity
	steps := 30 + rand.Intn(20) // More steps for smoother curves
	for i := 1; i <= steps; i++ {
		t := float64(i) / float64(steps)
		
		// S-curve velocity (starts slow, fast in middle, slows down at end)
		sCurve := t * t * (3 - 2*t)
		curX := fx1 + (fx2-fx1)*sCurve
		
		// Add vertical jitter (human hand isn't perfectly horizontal)
		jitter := (rand.Float64() - 0.5) * 4.0 // Increased jitter
		curY := fy1 + (fy2-fy1)*sCurve + jitter
		
		if err := chromedp.Run(s.MainCtx, chromedp.MouseEvent("mouseMoved", curX, curY, chromedp.Button("left"))); err != nil { return err }
		
		// Variable sleep to avoid fixed frequency detection
		sleepMs := 8 + rand.Intn(12)
		time.Sleep(time.Duration(sleepMs) * time.Millisecond)
	}

	time.Sleep(200 * time.Millisecond) // Pause before release

	// 3. Release
	return chromedp.Run(s.MainCtx, chromedp.MouseEvent("mouseReleased", fx2, fy2, chromedp.Button("left")))
}

// Action represents a single step in a batch execution.
type Action struct {
	Type   string  `json:"type"`   // "click", "drag", "type", "wait", "press"
	X      float64 `json:"x"`
	Y      float64 `json:"y"`
	X2     float64 `json:"x2"`
	Y2     float64 `json:"y2"`
	Text   string  `json:"text"`
	Key    string  `json:"key"`
	NodeID string  `json:"nodeId"`
	Wait   int     `json:"wait"`   // ms
}

// ExecuteBatch runs a sequence of actions with randomized delays.
func (s *Session) ExecuteBatch(actions []Action) error {
	for _, a := range actions {
		var err error
		switch strings.ToLower(a.Type) {
		case "navigate":
			err = s.Navigate(a.Text)
		case "click":
			if a.NodeID != "" {
				err = s.Click(a.NodeID)
			} else {
				err = s.ClickXY(a.X, a.Y, "")
			}
		case "drag":
			err = s.DragXY(a.X, a.Y, a.X2, a.Y2, a.NodeID)
		case "type":
				if err := chromedp.Run(s.Ctx, chromedp.SendKeys("", a.Text)); err != nil {
					return err
				}
		case "press":
			err = s.PressKey(a.Key)
		case "clear":
			if err := s.PressKey("Ctrl+A"); err != nil {
				return err
			}
			err = s.PressKey("Delete")
		case "wait":
			time.Sleep(time.Duration(a.Wait) * time.Millisecond)
		}
		if err != nil {
			return fmt.Errorf("action %s failed: %v", a.Type, err)
		}
		
		// Humanoid inter-action delay (1-2s as requested)
		delay := 800 + rand.Intn(700)
		time.Sleep(time.Duration(delay) * time.Millisecond)
	}
	return nil
}

// TypeText focuses a node spatially and types text. Parses \n as Enter key presses.
func (s *Session) TypeText(nodeID string, text string) error {
	fmt.Fprintf(os.Stderr, "      [TypeText] Received: %s\n", text)
	if nodeID != "" {
		if err := s.Click(nodeID); err != nil {
			return err
		}
	}
	
	lines := strings.Split(text, "\n")
	for i, line := range lines {
		if i > 0 {
			if err := s.PressKey("Enter"); err != nil {
				return err
			}
		}
		for _, r := range line {
			if err := chromedp.Run(s.Ctx, chromedp.KeyEvent(string(r))); err != nil {
				return err
			}
		}
	}
	return nil
}

// TypeNatural focuses a node and types text with human-like delays.
func (s *Session) TypeNatural(nodeID string, text string) error {
	if nodeID != "" {
		if err := s.Click(nodeID); err != nil {
			return err
		}
	}
	
	lines := strings.Split(text, "\n")
	for i, line := range lines {
		if i > 0 {
			if err := s.PressKey("Enter"); err != nil {
				return err
			}
			time.Sleep(time.Duration(200+rand.Intn(300)) * time.Millisecond)
		}
		for _, r := range line {
			// Base delay
			delay := 50 + rand.Intn(100) // 50-150ms
			
			// Extra delay for space
			if r == ' ' {
				delay += 100 + rand.Intn(100) // 150-250ms total
			}
			
			// Delay for caps
			if r >= 'A' && r <= 'Z' {
				delay += 50 + rand.Intn(50) // 100-200ms total
			}
			
			// Delay for symbols (not alphanumeric or space)
			if !((r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9')) && r != ' ' {
				delay += 70 + rand.Intn(70)
			}
			
			time.Sleep(time.Duration(delay) * time.Millisecond)
			
			// Typo logic (5% chance)
			if rand.Intn(100) < 5 {
				typoType := rand.Intn(2)
				var typoChar string
				if typoType == 0 {
					// Adjacent key
					adjacent := map[rune]string{
						'a': "qwsz", 'b': "vghn", 'c': "xdfv", 'd': "ersfxc", 'e': "wsdr",
						'f': "rtgvcd", 'g': "tyhbvf", 'h': "yujnbg", 'i': "ujko", 'j': "uikmnh",
						'k': "ijlmj", 'l': "okpl", 'm': "njk", 'n': "bhjm", 'o': "iklp",
						'p': "ol", 'q': "wa", 'r': "edtf", 's': "wedxza", 't': "rfyg",
						'u': "yhjk", 'v': "cfgb", 'w': "qase", 'x': "zsdc", 'y': "tghu", 'z': "asx",
					}
					if adj, ok := adjacent[r]; ok {
						typoChar = string(adj[rand.Intn(len(adj))])
					} else {
						typoChar = string(r)
					}
				} else {
					// Double type
					typoChar = string(r)
				}
				
				// Type the typo
				if err := chromedp.Run(s.Ctx, chromedp.KeyEvent(typoChar)); err != nil {
					return err
				}
				
				// Pause (realization)
				time.Sleep(time.Duration(300+rand.Intn(400)) * time.Millisecond)
				
				// Backspace
				if err := s.PressKey("Backspace"); err != nil {
					return err
				}
				
				// Pause before correction
				time.Sleep(time.Duration(100+rand.Intn(200)) * time.Millisecond)
			}
			
			if err := chromedp.Run(s.Ctx, chromedp.KeyEvent(string(r))); err != nil {
				return err
			}
		}
	}
	return nil
}


// JSClick is the fallback for nodes without physical layout.
func (s *Session) JSClick(nodeID string) error {
	id, err := strconv.Atoi(nodeID)
	if err != nil {
		return fmt.Errorf("invalid node ID: %s", nodeID)
	}
	
	return chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		obj, err := dom.ResolveNode().WithBackendNodeID(cdp.BackendNodeID(id)).Do(ctx)
		if err != nil { return err }
		
		_, _, err = runtime.CallFunctionOn("function() { if (this.tagName === 'A') { window.location.href = this.href; } else { this.click(); } }").
			WithObjectID(obj.ObjectID).Do(ctx)
		return err
	}))
}

// Scroll scrolls the page in the specified direction.
func (s *Session) Scroll(direction string, amount int) error {
	var x, y int
	switch direction {
	case "down":
		y = amount
	case "up":
		y = -amount
	}
	
	return chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		script := fmt.Sprintf("window.scrollBy(%d, %d)", x, y)
		var res interface{}
		return chromedp.Evaluate(script, &res).Do(ctx)
	}))
}

// PressKey presses a single keyboard key.
func (s *Session) PressKey(key string) error {
	return chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {

		
		var keyStr, codeStr string
		var vKey int
		switch key {
		case "Backspace":
			keyStr = "Backspace"
			codeStr = "Backspace"
			vKey = 8
		case "Enter":
			keyStr = "Enter"
			codeStr = "Enter"
			vKey = 13
		case "Tab":
			keyStr = "Tab"
			codeStr = "Tab"
			vKey = 9
		case "Delete":
			keyStr = "Delete"
			codeStr = "Delete"
			vKey = 46
		case "Ctrl+A":
			// Send Ctrl down, A down, A up, Ctrl up
			err := input.DispatchKeyEvent(input.KeyDown).WithKey("Control").WithCode("ControlLeft").WithWindowsVirtualKeyCode(17).Do(ctx)
			if err != nil {
				return err
			}
			err = input.DispatchKeyEvent(input.KeyDown).WithKey("a").WithCode("KeyA").WithWindowsVirtualKeyCode(65).WithModifiers(input.Modifier(2)).Do(ctx)
			if err != nil {
				return err
			}
			err = input.DispatchKeyEvent(input.KeyUp).WithKey("a").WithCode("KeyA").WithWindowsVirtualKeyCode(65).WithModifiers(input.Modifier(2)).Do(ctx)
			if err != nil {
				return err
			}
			return input.DispatchKeyEvent(input.KeyUp).WithKey("Control").WithCode("ControlLeft").WithWindowsVirtualKeyCode(17).Do(ctx)
		default:
			// If it's a single character, send it as a key event directly
			if len(key) == 1 {
				return chromedp.Run(s.Ctx, chromedp.KeyEvent(key))
			}
			keyStr = key
			codeStr = key
			vKey = 0
		}
		
		if keyStr == "Enter" {
			// Send KeyDown via CDP
			err := input.DispatchKeyEvent(input.KeyDown).WithKey(keyStr).WithCode(codeStr).WithWindowsVirtualKeyCode(int64(vKey)).WithUnmodifiedText("\r").WithText("\r").Do(ctx)
			if err != nil {
				return err
			}
			
			// Dispatch keypress and input events and insert newline via JS
			var res interface{}
			err = chromedp.Evaluate(`
				var el = document.activeElement;
				if (el && el.isContentEditable) {
					// Dispatch keypress
					var evKP = new KeyboardEvent('keypress', {bubbles: true, cancelable: true, key: 'Enter', code: 'Enter', keyCode: 13, which: 13});
					el.dispatchEvent(evKP);
					
					// Dispatch input
					var evInput = new InputEvent('input', {bubbles: true, cancelable: true, data: null, inputType: 'insertParagraph'});
					el.dispatchEvent(evInput);
					
					// Insert line break
					// document.execCommand('insertLineBreak');
				}
			`, &res).Do(ctx)
			if err != nil {
				return err
			}
			
			// Send KeyUp via CDP
			return input.DispatchKeyEvent(input.KeyUp).WithKey(keyStr).WithCode(codeStr).WithWindowsVirtualKeyCode(int64(vKey)).WithUnmodifiedText("\r").WithText("\r").Do(ctx)
		}
		
		// Send key down for other keys
		p := input.DispatchKeyEvent(input.KeyDown).WithKey(keyStr).WithCode(codeStr)
		if vKey != 0 {
			p = p.WithWindowsVirtualKeyCode(int64(vKey))
		}
		err := p.Do(ctx)
		if err != nil {
			return err
		}
		
		// Send key up for other keys
		p = input.DispatchKeyEvent(input.KeyUp).WithKey(keyStr).WithCode(codeStr)
		if vKey != 0 {
			p = p.WithWindowsVirtualKeyCode(int64(vKey))
		}
		return p.Do(ctx)
	}))
}

// TakeNodeScreenshot captures a screenshot of a specific element.
func (s *Session) TakeNodeScreenshot(nodeID string, path string) error {
	id, err := strconv.Atoi(nodeID)
	if err != nil {
		return fmt.Errorf("invalid node ID: %s", nodeID)
	}
	
	var buf []byte
	err = chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		obj, err := dom.ResolveNode().WithBackendNodeID(cdp.BackendNodeID(id)).Do(ctx)
		if err != nil { return err }
		nodeID, err := dom.RequestNode(obj.ObjectID).Do(ctx)
		if err != nil { return err }
		return chromedp.Screenshot([]cdp.NodeID{nodeID}, &buf, chromedp.ByNodeID).Do(ctx)
	}))
	if err != nil {
		return err
	}
	return os.WriteFile(path, buf, 0644)
}

// GetFrames returns a list of all browser targets (frames/iframes) with their target IDs.
// Uses target.GetTargets() so the returned IDs work with SwitchToFrame.
func (s *Session) GetFrames() (string, error) {
	var res []string
	
	// First get targets
	infos, err := cdpTarget.GetTargets().Do(s.Ctx)
	if err == nil {
		for _, info := range infos {
			line := fmt.Sprintf("%s | %s | %s", info.TargetID, info.Type, info.URL)
			fmt.Fprintf(os.Stderr, "      [Browser] Target: %s\n", line)
			res = append(res, line)
		}
	}

	// Also try to get frame tree for sub-frames
	var tree *cdpPage.FrameTree
	err = chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		var err error
		tree, err = cdpPage.GetFrameTree().Do(ctx)
		return err
	}))
	
	if err == nil && tree != nil {
		var walk func(*cdpPage.FrameTree)
		walk = func(t *cdpPage.FrameTree) {
			line := fmt.Sprintf("%s | frame | %s", t.Frame.ID, t.Frame.URL)
			fmt.Fprintf(os.Stderr, "      [Browser] Frame: %s\n", line)
			res = append(res, line)
			for _, child := range t.ChildFrames {
				walk(child)
			}
		}
		walk(tree)
	}

	return strings.Join(res, "\n"), nil
}

// SwitchToFrame changes the active target to a specific frame ID.
func (s *Session) SwitchToFrame(targetID string) error {
	id := cdpTarget.ID(targetID)
	ctx, cancel := chromedp.NewContext(s.Ctx, chromedp.WithTargetID(id))
	if err := chromedp.Run(ctx); err != nil {
		cancel()
		return err
	}
	s.Ctx = ctx
	s.Cancel = cancel
	s.ActiveTargetID = id
	return nil
}

// SwitchToMainFrame restores the active target to the main page.
func (s *Session) SwitchToMainFrame() error {
	if s.ActiveTargetID == s.MainTargetID {
		return nil
	}
	ctx, cancel := chromedp.NewContext(s.Ctx, chromedp.WithTargetID(s.MainTargetID))
	if err := chromedp.Run(ctx); err != nil {
		cancel()
		return err
	}
	s.Ctx = ctx
	s.Cancel = cancel
	s.ActiveTargetID = s.MainTargetID
	return nil
}

// InspectNode returns deep metadata for a single node.
// It first checks LastAom for a cached node, but if not found, it creates
// a fresh PrunedNode directly from Chrome's DOM API. This allows inspecting
// ANY element on the page, not just those that survived AOM pruning.
func (s *Session) InspectNode(backendID int64) (*PrunedNode, error) {
	// Try to find in cached AOM first
	var target *PrunedNode
	var find func([]*PrunedNode)
	find = func(ns []*PrunedNode) {
		for _, n := range ns {
			if n.BackendID == backendID { target = n; return }
			find(n.Children)
		}
	}
	find(s.LastAom)

	// If not in AOM, create a fresh node — don't block on pruner output
	if target == nil {
		target = &PrunedNode{
			BackendID: backendID,
			NodeID:    fmt.Sprintf("%d", backendID),
			Children:  []*PrunedNode{},
		}
	}

	err := chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		// Get bounding box
		model, err := dom.GetBoxModel().WithBackendNodeID(cdp.BackendNodeID(backendID)).Do(ctx)
		if err == nil {
			target.X = int64(model.Content[0])
			target.Y = int64(model.Content[1])
			target.W = int64(model.Width)
			target.H = int64(model.Height)
		}
		// Get node description for tag name, attributes, styles
		nodeInfo, err := dom.DescribeNode().WithBackendNodeID(cdp.BackendNodeID(backendID)).Do(ctx)
		if err == nil {
			target.Role = nodeInfo.NodeName
			if target.Attrs == nil {
				target.Attrs = make(map[string]string)
			}
			for i := 0; i+1 < len(nodeInfo.Attributes); i += 2 {
				target.Attrs[nodeInfo.Attributes[i]] = nodeInfo.Attributes[i+1]
			}
			// Try to get computed styles
			styles, _, err := css.GetComputedStyleForNode(nodeInfo.NodeID).Do(ctx)
			if err == nil {
				var res []string
				for _, st := range styles {
					res = append(res, fmt.Sprintf("%s:%s", st.Name, st.Value))
				}
				target.Style = strings.Join(res, "; ")
			}
		}
		return nil
	}))
	return target, err
}

// QuerySelector uses a single JS eval to find elements by CSS selector.
// Returns a list of matches with bounding boxes and metadata — no DOM tree traversal needed.
// This is the surgical fallback for discovering elements that the AOM pruner strips
// (e.g. generic divs like jspaint's color palette or toolbox).
func (s *Session) QuerySelector(selector string) (string, error) {
	var results []map[string]interface{}

	err := chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		// Find nodes by selector
		script := fmt.Sprintf(`
			(function() {
				function findInShadows(root, selector) {
					let matches = Array.from(root.querySelectorAll(selector));
					let all = root.querySelectorAll('*');
					for (let el of all) {
						if (el.shadowRoot) {
							matches = matches.concat(findInShadows(el.shadowRoot, selector));
						}
					}
					return matches;
				}
				var els = findInShadows(document, %q);
				var ids = [];
				for (var i = 0; i < els.length && i < 50; i++) {
					if (!window._mcp_nodes) window._mcp_nodes = [];
					window._mcp_nodes.push(els[i]);
					ids.push(window._mcp_nodes.length - 1);
				}
				return ids;
			})()
		`, selector)

		var indices []int
		if err := chromedp.Evaluate(script, &indices).Do(ctx); err != nil {
			return err
		}

		for _, idx := range indices {
			// Resolve each index back to a backend ID
			resolveScript := fmt.Sprintf("window._mcp_nodes[%d]", idx)
			obj, _, err := runtime.Evaluate(resolveScript).Do(ctx)
			if err != nil { continue }

			nodeInfo, err := dom.DescribeNode().WithObjectID(obj.ObjectID).Do(ctx)
			if err != nil { continue }

			// Get bounding box
			model, err := dom.GetBoxModel().WithObjectID(obj.ObjectID).Do(ctx)
			var x, y, w, h int64
			if err == nil {
				x, y, w, h = int64(model.Content[0]), int64(model.Content[1]), int64(model.Width), int64(model.Height)
			}

			// Attributes
			attrs := make(map[string]string)
			for i := 0; i+1 < len(nodeInfo.Attributes); i += 2 {
				attrs[nodeInfo.Attributes[i]] = nodeInfo.Attributes[i+1]
			}

			results = append(results, map[string]interface{}{
				"backendNodeId": int64(nodeInfo.BackendNodeID),
				"tag":           nodeInfo.NodeName,
				"id":            attrs["id"],
				"class":         attrs["class"],
				"title":         attrs["title"],
				"x":             x,
				"y":             y,
				"w":             w,
				"h":             h,
			})
		}
		
		// Clean up
		return chromedp.Evaluate("window._mcp_nodes = []", nil).Do(ctx)
	}))

	if err != nil {
		return "", fmt.Errorf("querySelectorAll(%s): %w", selector, err)
	}

	data, _ := json.Marshal(results)
	return string(data), nil
}

// ClickByName finds a node by its Name attribute and clicks it.
func (s *Session) ClickByName(name string) error {
	var targetID int64
	var find func([]*PrunedNode)
	find = func(ns []*PrunedNode) {
		for _, n := range ns {
			if n.Name == name { targetID = n.BackendID; return }
			find(n.Children)
		}
	}
	find(s.LastAom)
	if targetID == 0 { return fmt.Errorf("node with name %q not found", name) }
	return s.Click(fmt.Sprintf("%d", targetID))
}

// TypeTextByPlaceholder finds an input by its placeholder and types text.
func (s *Session) TypeTextByPlaceholder(placeholder string, text string) error {
	var targetID int64
	var find func([]*PrunedNode)
	find = func(ns []*PrunedNode) {
		for _, n := range ns {
			if n.Attrs["placeholder"] == placeholder { targetID = n.BackendID; return }
			find(n.Children)
		}
	}
	find(s.LastAom)
	if targetID == 0 { return fmt.Errorf("input with placeholder %q not found", placeholder) }
	return s.TypeText(fmt.Sprintf("%d", targetID), text)
}
// QueryArea finds elements within a specific screen area (x1, y1) to (x2, y2).
func (s *Session) QueryArea(x1, y1, x2, y2 float64) (string, error) {
	var results []map[string]interface{}

	err := chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		script := fmt.Sprintf(`
			(function() {
				var els = document.querySelectorAll('*');
				var ids = [];
				for (var i = 0; i < els.length; i++) {
					var rect = els[i].getBoundingClientRect();
					if (rect.left >= %f && rect.top >= %f && rect.right <= %f && rect.bottom <= %f) {
						if (!window._mcp_nodes) window._mcp_nodes = [];
						window._mcp_nodes.push(els[i]);
						ids.push(window._mcp_nodes.length - 1);
					}
					if (ids.length >= 50) break;
				}
				return ids;
			})()
		`, x1, y1, x2, y2)

		var indices []int
		if err := chromedp.Evaluate(script, &indices).Do(ctx); err != nil {
			return err
		}

		for _, idx := range indices {
			resolveScript := fmt.Sprintf("window._mcp_nodes[%d]", idx)
			obj, _, err := runtime.Evaluate(resolveScript).Do(ctx)
			if err != nil { continue }

			nodeInfo, err := dom.DescribeNode().WithObjectID(obj.ObjectID).Do(ctx)
			if err != nil { continue }

			model, err := dom.GetBoxModel().WithObjectID(obj.ObjectID).Do(ctx)
			var x, y, w, h int64
			if err == nil {
				x, y, w, h = int64(model.Content[0]), int64(model.Content[1]), int64(model.Width), int64(model.Height)
			}

			attrs := make(map[string]string)
			for i := 0; i+1 < len(nodeInfo.Attributes); i += 2 {
				attrs[nodeInfo.Attributes[i]] = nodeInfo.Attributes[i+1]
			}

			results = append(results, map[string]interface{}{
				"backendNodeId": int64(nodeInfo.BackendNodeID),
				"tag":           nodeInfo.NodeName,
				"id":            attrs["id"],
				"class":         attrs["class"],
				"title":         attrs["title"],
				"x":             x,
				"y":             y,
				"w":             w,
				"h":             h,
			})
		}
		
		return chromedp.Evaluate("window._mcp_nodes = []", nil).Do(ctx)
	}))

	if err != nil {
		return "", fmt.Errorf("queryArea: %w", err)
	}

	data, _ := json.Marshal(results)
	return string(data), nil
}
