package browser

import (
	"encoding/json"
	"fmt"
	"log"
	"math/rand"
	"net"
	"os"
	"os/exec"
	"runtime"
	"sync"
	"time"
)

type GhostSession struct {
	conn       net.Conn
	cmd        *exec.Cmd
	mu         sync.Mutex
	aomChan    chan string
}

func NewGhostSession(proxyServer string) (*GhostSession, error) {
	log.Printf("[Ghost] Starting socket listener...")
	var ln net.Listener
	var err error
	if runtime.GOOS == "linux" {
		os.Remove("/tmp/ghost.sock")
		ln, err = net.Listen("unix", "/tmp/ghost.sock")
		os.Chmod("/tmp/ghost.sock", 0666)
	} else {
		ln, err = net.Listen("tcp4", "0.0.0.0:9998")
	}
	if err != nil {
		return nil, fmt.Errorf("failed to listen: %v", err)
	}

	s := &GhostSession{
		aomChan: make(chan string, 1),
	}

	go func() {
		// Cleanup stale processes
		if runtime.GOOS == "windows" {
			exec.Command("taskkill", "/F", "/IM", "chrome.exe", "/T").Run()
			exec.Command("taskkill", "/F", "/IM", "ghost_bridge_host.exe", "/T").Run()
		}
		time.Sleep(1 * time.Second)

		// Enforce enterprise policy for headless/standard Chrome extension installation
		extPath := "c:\\Users\\visse\\OneDrive\\Documentos\\Kimi Code\\velocity-workspace\\browsing\\extension\\src"
		if runtime.GOOS == "linux" {
			extPath = "/app/extension/src"
		}
		if err := EnforceExtensionPolicy("niloignjhdlhfpepaiccfkoipaoofpbd", extPath, proxyServer); err != nil {
			log.Printf("[Ghost] Failed to set enterprise policy: %v", err)
		}

		log.Println("[Ghost] Waiting for Chrome to load extension and connect...")
		// Use Standard Consumer Google Chrome
		chromePath := "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
		if runtime.GOOS == "linux" {
			chromePath = "/usr/bin/google-chrome"
		}
		userDataDir := "c:\\Users\\visse\\OneDrive\\Documentos\\Kimi Code\\velocity-workspace\\browsing\\chrome_profile"
		if runtime.GOOS == "linux" {
			userDataDir = "/app/ghost_chrome_profile"
		}
		
		ua := "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36"
		if runtime.GOOS == "linux" {
			ua = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36"
		}

		s.cmd = exec.Command(chromePath,
			"--user-data-dir="+userDataDir,
			"--user-agent="+ua,
			"--no-first-run",
			"--no-default-browser-check",
			"--start-maximized",
			"--load-extension="+extPath,
			"--disable-session-crashed-bubble",
			"--disable-infobars",
			"--disable-extensions-except="+extPath,
			"--window-size=1920,1080",
			"--window-position=0,0",
			"--force-renderer-accessibility",
			"--lang=en-US",
			"--disable-gpu-sandbox",
			"--password-store=basic",
			"--use-gl=egl",
			"--enable-webgl",
			"--ignore-gpu-blocklist",
			"--disable-dev-shm-usage",
			"--remote-debugging-port=9222",
			"--no-proxy-server",
			"--proxy-bypass-list=<-loopback>",
			"--disable-blink-features=AutomationControlled",
			"--no-sandbox",
			"--disable-setuid-sandbox",
			"--password-store=basic",
			"--disable-dev-shm-usage",
			"--test-type",
			"--enable-logging",
			"--v=1",
		)
		log.Printf("[Ghost] Launching Chrome: %v", s.cmd.Args)
		s.cmd.Stdout = os.Stdout
		s.cmd.Stderr = os.Stderr
		if err := s.cmd.Start(); err != nil {
			log.Printf("[Ghost] Failed to start Chrome: %v", err)
			return
		}

		// Wait for Native Host to connect
		conn, err := ln.Accept()
		if err != nil {
			log.Printf("Socket accept failed: %v", err)
			return
		}
		s.conn = conn
		log.Println("Ghost Bridge Connected!")

		// Handle incoming messages
		decoder := json.NewDecoder(conn)
		for {
			var msg map[string]interface{}
			if err := decoder.Decode(&msg); err != nil {
				log.Printf("[Ghost] Bridge connection error: %v", err)
				break
			}
			log.Printf("[Ghost] Received from Bridge: %v", msg["type"])
			switch msg["type"] {
			case "AOM_RESULT":
				if data, ok := msg["data"].(string); ok {
					s.aomChan <- data
				}
			case "TRAFFIC_LOG":
				if data, ok := msg["data"].(map[string]interface{}); ok {
					dir := data["direction"]
					url := data["url"]
					fmt.Printf("[Traffic] %v %v\n", dir, url)
					if headers, ok := data["headers"].([]interface{}); ok {
						for _, h := range headers {
							if header, ok := h.(map[string]interface{}); ok {
								fmt.Printf("          %v: %v\n", header["name"], header["value"])
							}
						}
					}
				}
			}
		}
	}()

	// Wait for connection (timeout 300s)
	start := time.Now()
	for s.conn == nil {
		if time.Since(start) > 300*time.Second {
			return nil, fmt.Errorf("ghost bridge connection timeout")
		}
		time.Sleep(1 * time.Second)
	}

	return s, nil
}

func (s *GhostSession) Navigate(url string) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	req := map[string]string{"type": "NAVIGATE", "url": url}
	payload, _ := json.Marshal(req)
	s.conn.Write(append(payload, '\n'))
	time.Sleep(2 * time.Second) // Give it a moment
	return nil
}

func (s *GhostSession) Click(x, y int) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	req := map[string]interface{}{"type": "CLICK", "x": x, "y": y}
	payload, _ := json.Marshal(req)
	s.conn.Write(append(payload, '\n'))
	return nil
}

func (s *GhostSession) MoveSmooth(x, y int) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	req := map[string]interface{}{"type": "MOVE_SMOOTH", "x": x, "y": y}
	payload, _ := json.Marshal(req)
	s.conn.Write(append(payload, '\n'))
	return nil
}

func (s *GhostSession) MouseDown(x, y int) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	req := map[string]interface{}{"type": "MOUSE_DOWN", "x": x, "y": y}
	payload, _ := json.Marshal(req)
	s.conn.Write(append(payload, '\n'))
	return nil
}

func (s *GhostSession) MouseUp(x, y int) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	req := map[string]interface{}{"type": "MOUSE_UP", "x": x, "y": y}
	payload, _ := json.Marshal(req)
	s.conn.Write(append(payload, '\n'))
	return nil
}

// MoveBezier simulates organic human mouse movement using a simple quadratic Bezier curve
func (s *GhostSession) MoveBezier(startX, startY, endX, endY int) error {
	// Pick a random control point to create a curve
	// Add some randomness to the control point for each move
	controlX := (startX+endX)/2 + rand.Intn(100) - 50
	controlY := (startY+endY)/2 + rand.Intn(100) - 50

	steps := 15 + rand.Intn(10) // Variable number of steps
	for i := 0; i <= steps; i++ {
		t := float64(i) / float64(steps)
		// Quadratic Bezier formula: (1-t)^2*P0 + 2(1-t)t*P1 + t^2*P2
		x := int((1-t)*(1-t)*float64(startX) + 2*(1-t)*t*float64(controlX) + t*t*float64(endX))
		y := int((1-t)*(1-t)*float64(startY) + 2*(1-t)*t*float64(controlY) + t*t*float64(endY))

		// Add jitter (sub-pixel/small pixel noise)
		x += rand.Intn(3) - 1
		y += rand.Intn(3) - 1

		req := map[string]interface{}{"type": "MOVE", "x": x, "y": y}
		payload, _ := json.Marshal(req)
		s.conn.Write(append(payload, '\n'))
		
		// Randomize speed slightly
		time.Sleep(time.Duration(10+rand.Intn(15)) * time.Millisecond)
	}
	return nil
}

func (s *GhostSession) PerformNativeAction(identifier string, action string, value string) error {
	msg := map[string]string{
		"type":       "NATIVE_ACTION",
		"identifier": identifier,
		"action":     action,
		"value":      value,
	}
	payload, _ := json.Marshal(msg)
	s.conn.Write(append(payload, '\n'))
	return nil
}

func (s *GhostSession) GetAom() (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	req := map[string]string{"type": "GET_AOM"}
	payload, _ := json.Marshal(req)
	s.conn.Write(append(payload, '\n'))

	select {
	case result := <-s.aomChan:
		return result, nil
	case <-time.After(15 * time.Second):
		return "", fmt.Errorf("AOM request timed out")
	}
}

func (s *GhostSession) Close() {
	if s.conn != nil { s.conn.Close() }
	if s.cmd != nil { s.cmd.Process.Kill() }
}
