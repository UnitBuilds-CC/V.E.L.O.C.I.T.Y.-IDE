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
	fmt.Println("Amazon Stress Test starting...")

	// 1. Setup Managed Browser with Stealth Flags
	allocCtx, cancel := chromedp.NewExecAllocator(context.Background(), 
		append(chromedp.DefaultExecAllocatorOptions[:], 
			chromedp.NoSandbox,
			chromedp.Flag("disable-setuid-sandbox", true),
			chromedp.Flag("headless", "new"), // Use modern headless mode
			chromedp.WindowSize(1920, 1080),
			chromedp.UserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"),
		)...)
	defer cancel()

	ctx, cancel := chromedp.NewContext(allocCtx)
	defer cancel()

	sess := &browser.Session{
		Ctx:    ctx,
		Cancel: cancel,
	}

	// 2. Navigate to Amazon
	fmt.Println("Navigating to Amazon...")
	sess.Navigate("https://www.amazon.com")
	
	// Wait for the search box to be visible - this is a real stealth check
	err := chromedp.Run(sess.Ctx, chromedp.WaitVisible("#twotabsearchtextbox", chromedp.ByID))
	if err != nil {
		fmt.Printf("  Warning: Initial WaitVisible failed: %v\n", err)
	}
	
	sess.WaitForStability(0)
	sess.TakeScreenshot("amazon_1_home.png")

	// 3. Search
	fmt.Println("Searching for 'Mechanical Keyboard'...")
	aom, _ := sess.GetAom(browser.AomConfig{})
	searchBox := findNode(sess.LastAom, []string{"textbox", "combobox", "searchbox", "Search"}, "Search")
	if searchBox == nil {
		fmt.Println("  Search box not found! AOM Snippet:")
		if len(aom) > 2000 { fmt.Println(aom[:2000]) } else { fmt.Println(aom) }
	} else {
		fmt.Printf("  Found search box: %s (%s)\n", searchBox.Name, searchBox.Role)
		sess.TypeText(searchBox.NodeID, "Mechanical Keyboard\r") // Append Enter to the text
		
		// Optional: Find search button
		searchBtn := findNode(sess.LastAom, []string{"button"}, "Go")
		if searchBtn == nil { searchBtn = findNode(sess.LastAom, []string{"button"}, "Search") }
		if searchBtn != nil { sess.JSClick(searchBtn.NodeID) }
	}
	fmt.Println("  Waiting for results to render (5s)...")
	time.Sleep(5 * time.Second)
	sess.WaitForStability(0)
	sess.TakeScreenshot("amazon_2_results.png")

	// 4. Filter (e.g. click a brand or category)
	fmt.Println("Applying filter...")
	sess.GetAom(browser.AomConfig{})
	filter := findNode(sess.LastAom, []string{"link"}, "Logitech") // Try to filter by Logitech
	if filter != nil {
		sess.JSClick(filter.NodeID)
		sess.WaitForStability(0)
	}
	sess.TakeScreenshot("amazon_3_filter.png")

	// 5. Select Item
	fmt.Println("Selecting item...")
	sess.GetAom(browser.AomConfig{})
	item := findNode(sess.LastAom, []string{"link"}, "Keyboard") // Find first link containing Keyboard
	if item != nil {
		sess.JSClick(item.NodeID)
		sess.WaitForStability(0)
	}
	sess.TakeScreenshot("amazon_4_item.png")

	// 6. Select Related
	fmt.Println("Selecting related...")
	sess.GetAom(browser.AomConfig{})
	// Look for 'Frequently bought together' or 'Add to Cart'
	related := findNode(sess.LastAom, []string{"button"}, "Add to Cart")
	if related != nil {
		fmt.Println("  Found 'Add to Cart' for the related/current item.")
		sess.JSClick(related.NodeID)
		sess.WaitForStability(0)
	}
	sess.TakeScreenshot("amazon_5_cart.png")

	fmt.Println("Amazon Stress Test finished.")
}
