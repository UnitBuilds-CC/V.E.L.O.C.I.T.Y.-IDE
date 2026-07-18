package main

import (
	"fmt"
	"log"
	"strings"
	"time"

	"github.com/chromedp/chromedp"
	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

func main() {
	fmt.Println("=== ADVANCED STEALTH TEST (G2.COM) ===")
	s, err := browser.NewManagedSession()
	if err != nil {
		log.Fatalf("Failed to start browser: %v", err)
	}
	defer s.Close()

	url := "https://www.g2.com"
	fmt.Printf("[1] Navigating to %s (with 15s timeout)...\n", url)
	start := time.Now()
	
	// Navigate with a shorter timeout to avoid hanging on blocker
	navErr := make(chan error, 1)
	go func() {
		navErr <- s.Navigate(url)
	}()

	select {
	case err := <-navErr:
		if err != nil {
			fmt.Printf("      [Warning] Navigation finished with error: %v\n", err)
		}
	case <-time.After(15 * time.Second):
		fmt.Println("      [Timeout] Navigation timed out, likely hit a blocker. Proceeding to stealth phase.")
	}

	// [SMART BYPASS] Behavioral Emulation
	fmt.Println("[2] Simulating human behavioral patterns...")
	for i := 0; i < 10; i++ {
		x := 100.0 + float64(i*40)
		y := 100.0 + float64(i*25)
		chromedp.Run(s.Ctx, chromedp.MouseEvent("mouseMoved", x, y))
		time.Sleep(50 * time.Millisecond)
	}

	fmt.Println("[3] Polling for bypass (Clean Sweep active)...")
	for i := 0; i < 20; i++ {
		aom, _ := s.GetAom(browser.AomConfig{})
		aomLower := strings.ToLower(aom)
		if strings.Contains(aomLower, "software") || strings.Contains(aomLower, "categories") {
			fmt.Printf("\n      [Success] BYPASSED G2 in %v!\n", time.Since(start))
			break
		}
		
		if strings.Contains(aomLower, "datadome") || strings.Contains(aomLower, "cloudflare") {
			fmt.Printf("      ...still challenged (AOM includes security markers)\n")
		} else if len(aom) > 100 {
			fmt.Printf("      ...neutral state (AOM length: %d)\n", len(aom))
		}
		
		time.Sleep(2 * time.Second)
	}

	fmt.Println("[4] Capturing evidence...")
	s.TakeScreenshot("stealth_test_result.png")
	
	finalAom, _ := s.GetAom(browser.AomConfig{})
	fmt.Printf("\n=== FINAL AOM (Truncated) ===\n%s\n", truncate(finalAom, 500))
	fmt.Println("\n=== TEST COMPLETE ===")
}

func truncate(s string, n int) string {
	if len(s) <= n { return s }
	return s[:n] + "..."
}
