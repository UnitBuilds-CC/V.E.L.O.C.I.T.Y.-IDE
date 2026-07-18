package main

import (
	"encoding/json"
	"fmt"
	"log"
	"os/exec"
	"strings"
	"time"

	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

func takeScreenshot(name string) {
	fmt.Printf("    [Ghost] Screenshot: %s.png\n", name)
	exec.Command("mkdir", "-p", "/app/screenshots").Run()
	exec.Command("scrot", "/app/screenshots/"+name+".png").Run()
}

type AomNode struct {
	Role     string    `json:"role"`
	Name     string    `json:"name"`
	X        int       `json:"x"`
	Y        int       `json:"y"`
	W        int       `json:"w"`
	H        int       `json:"h"`
	Children []AomNode `json:"children"`
}

func findNode(node AomNode, nameMatch string) *AomNode {
	if strings.Contains(node.Name, nameMatch) {
		return &node
	}
	for _, child := range node.Children {
		if found := findNode(child, nameMatch); found != nil {
			return found
		}
	}
	return nil
}

func main() {
	fmt.Println("[Ghost] Starting Test Orchestrator...")
	// Initialize Ghost Session
	s, err := browser.NewGhostSession("")
	if err != nil {
		fmt.Printf("[Ghost] FATAL: Failed to start Ghost Session: %v\n", err)
		log.Fatalf("Failed to start Ghost Session: %v", err)
	}
	defer s.Close()

	// Maximize browser window
	fmt.Println("[Ghost] Maximizing browser window...")
	exec.Command("xdotool", "search", "--onlyvisible", "--class",
		"Google-chrome", "windowactivate", "windowsize", "1920", "1080").Run()
	time.Sleep(3 * time.Second)

	// 1. Diagnostic: check our identity via httpbin
	fmt.Println("[1] Identity check via httpbin.org...")
	s.Navigate("https://httpbin.org/get")
	time.Sleep(5 * time.Second)
	takeScreenshot("0_identity_check")

	// 2. Navigate directly to target — no warming needed with truthful identity
	fmt.Println("[2] Navigating to bot.sannysoft.com (identity audit)...")
	s.Navigate("https://bot.sannysoft.com")
	time.Sleep(15 * time.Second)
	takeScreenshot("1_bot_audit")

	// 3. Check AOM for DataDome challenge
	fmt.Println("[3] Checking AOM for DataDome challenge...")
	aomStr, err := s.GetAom()
	if err != nil {
		log.Printf("AOM retrieval failed: %v", err)
		takeScreenshot("2_aom_failed")
		return
	}

	var root AomNode
	if err := json.Unmarshal([]byte(aomStr), &root); err != nil {
		log.Printf("Failed to parse AOM: %v", err)
	}

	challenge := findNode(root, "DataDome Device Check")
	if challenge != nil {
		fmt.Println("    [Ghost] DataDome challenge detected! Attempting native bypass...")
		takeScreenshot("2_challenge_detected")

		s.PerformNativeAction("DataDome Device Check", "CLICK", "")
		time.Sleep(15 * time.Second)
		takeScreenshot("3_after_bypass")
	} else {
		fmt.Println("    [Ghost] No challenge detected — seamless pass!")
		takeScreenshot("2_seamless_pass")
	}

	fmt.Println("\n=== TEST COMPLETE ===")
}
