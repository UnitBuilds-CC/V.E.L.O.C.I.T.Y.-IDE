package main

import (
	"context"
	"fmt"
	"strings"

	"github.com/chromedp/chromedp"
	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

func findNode(ns []*browser.PrunedNode, roles []string, namePart string) *browser.PrunedNode {
	for _, n := range ns {
		matchRole := false
		for _, r := range roles { if n.Role == r { matchRole = true; break } }
		
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
	fmt.Println("Wikipedia Autonomous Test starting...")

	// 1. Setup Managed Browser
	allocCtx, cancel := chromedp.NewExecAllocator(context.Background(), 
		append(chromedp.DefaultExecAllocatorOptions[:], 
			chromedp.NoSandbox,
			chromedp.Flag("disable-setuid-sandbox", true),
			chromedp.Flag("headless", "new"),
			chromedp.WindowSize(1280, 720),
		)...)
	defer cancel()

	ctx, cancel := chromedp.NewContext(allocCtx)
	defer cancel()

	sess := &browser.Session{
		Ctx:    ctx,
		Cancel: cancel,
	}

	// 2. Navigate to Wikipedia
	fmt.Println("Navigating to Wikipedia...")
	sess.Navigate("https://en.wikipedia.org/wiki/Main_Page")
	sess.WaitForStability(0)
	sess.TakeScreenshot("wiki_1_home.png")

	// 3. Search for 'Go (programming language)'
	fmt.Println("Searching for 'Go (programming language)'...")
	sess.GetAom(browser.AomConfig{})
	searchBox := findNode(sess.LastAom, []string{"searchbox", "textbox", "combobox"}, "Search")
	if searchBox != nil {
		fmt.Printf("  Found search box: %s\n", searchBox.NodeID)
		sess.TypeText(searchBox.NodeID, "Go (programming language)\r")
	} else {
		fmt.Println("  Search box not found in AOM. Falling back to ID...")
		chromedp.Run(sess.Ctx, chromedp.SendKeys("#searchInput", "Go (programming language)\r", chromedp.ByID))
	}
	
	sess.WaitForStability(0)
	sess.TakeScreenshot("wiki_2_results.png")

	// 4. Click a link in the article
	fmt.Println("Selecting a link in the article...")
	sess.GetAom(browser.AomConfig{})
	link := findNode(sess.LastAom, []string{"link"}, "Google") // Look for Google link in the Go article
	if link != nil {
		fmt.Printf("  Clicking link: %s (%s)\n", link.Name, link.NodeID)
		sess.JSClick(link.NodeID)
		sess.WaitForStability(0)
	}
	sess.TakeScreenshot("wiki_3_final.png")

	fmt.Println("Wikipedia Autonomous Test finished.")
}
