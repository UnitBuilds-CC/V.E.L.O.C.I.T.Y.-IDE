package main

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/chromedp/chromedp"
	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

func findNode(ns []*browser.PrunedNode, roles []string, namePart string) *browser.PrunedNode {
	for _, n := range ns {
		matchRole := false
		for _, r := range roles { if strings.EqualFold(n.Role, r) { matchRole = true; break } }
		
		if matchRole && (namePart == "" || strings.Contains(strings.ToLower(n.Name), strings.ToLower(namePart))) {
			return n
		}
		if child := findNode(n.Children, roles, namePart); child != nil {
			return child
		}
	}
	return nil
}

func main() {
	fmt.Println("=== GO HYBRID ENGINE BENCHMARK ===")
	start := time.Now()

	allocCtx, cancel := chromedp.NewExecAllocator(context.Background(), 
		append(chromedp.DefaultExecAllocatorOptions[:], 
			chromedp.NoSandbox,
			chromedp.Flag("headless", "new"),
			chromedp.WindowSize(1920, 1080),
			chromedp.UserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"),
		)...)
	defer cancel()

	ctx, cancel := chromedp.NewContext(allocCtx)
	defer cancel()

	sess := &browser.Session{Ctx: ctx, Cancel: cancel}

	// 1. Navigate (Base AOM)
	fmt.Println("[1] Navigating to Amazon (Frugal Mode)...")
	sess.Navigate("https://www.amazon.com")
	
	// Wait for search box explicitly to ensure page loaded
	err := chromedp.Run(sess.Ctx, chromedp.WaitVisible("#twotabsearchtextbox", chromedp.ByID))
	if err != nil {
		fmt.Printf("    - Error waiting for searchbox: %v\n", err)
	}

	sess.QuickWait(0)
	aom, _ := sess.GetAom(browser.AomConfig{})
	fmt.Printf("    - Base AOM Length: %d chars\n", len(aom))

	// 2. Search
	fmt.Println("[2] Searching for 'Logitech G915'...")
	searchBox := findNode(sess.LastAom, []string{"textbox", "searchbox", "combobox"}, "Search")
	if searchBox != nil {
		fmt.Printf("    - Found searchbox: %s\n", searchBox.NodeID)
		sess.TypeText(searchBox.NodeID, "Logitech G915\r")
	} else {
		fmt.Println("    - Searchbox NOT found in AOM. Falling back to ID...")
		chromedp.Run(sess.Ctx, chromedp.SendKeys("#twotabsearchtextbox", "Logitech G915\r", chromedp.ByID))
	}
	
	fmt.Println("    - Awaiting results...")
	sess.WaitUntilElementExists("link", "Logitech", 10 * time.Second)
	sess.QuickWait(0)

	// 3. Select First Result
	fmt.Println("[3] Selecting first result...")
	sess.GetAom(browser.AomConfig{})
	item := findNode(sess.LastAom, []string{"link"}, "Keyboard")
	if item == nil { item = findNode(sess.LastAom, []string{"link"}, "Logitech") }
	
	if item != nil {
		fmt.Printf("    - Clicking: %s\n", item.Name)
		sess.JSClick(item.NodeID)
	}
	
	fmt.Println("    - Awaiting product page...")
	sess.WaitUntilElementExists("button", "Add to Cart", 10 * time.Second)
	sess.QuickWait(0)

	// 4. THE DEEP DIVE (Targeted Inspection)
	fmt.Println("[4] Inspecting 'Add to Cart' button (Sensory Mode)...")
	sess.GetAom(browser.AomConfig{})
	btn := findNode(sess.LastAom, []string{"button"}, "Add to Cart")
	if btn != nil {
		fmt.Printf("    - Found Button Node: %d\n", btn.BackendID)
		
		deepNode, err := sess.InspectNode(btn.BackendID)
		if err != nil {
			fmt.Printf("    - Inspection Error: %v\n", err)
		} else {
			fmt.Println("=== SENSORY RESULTS ===")
			fmt.Printf("    - Coordinates: (%d, %d, %d, %d)\n", deepNode.X, deepNode.Y, deepNode.W, deepNode.H)
			fmt.Printf("    - Visual Style: %s\n", deepNode.Style)
		}
	} else {
		fmt.Println("    - 'Add to Cart' button not found in AOM.")
	}

	elapsed := time.Since(start)
	fmt.Printf("\n=== BENCHMARK COMPLETE ===\n")
	fmt.Printf("Total Time: %v\n", elapsed)
}
