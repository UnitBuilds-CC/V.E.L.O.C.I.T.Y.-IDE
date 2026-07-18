package main

import (
	"fmt"
	"log"
	"time"

	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

func main() {
	s, err := browser.NewManagedSession()
	if err != nil {
		log.Fatalf("Failed to start browser: %v", err)
	}
	defer s.Close()

	sites := []string{
		"https://www.wikipedia.org",
		"https://www.amazon.com",
		"https://www.nytimes.com",
	}

	for _, url := range sites {
		fmt.Printf("\n[Testing] %s\n", url)
		start := time.Now()
		
		if err := s.Navigate(url); err != nil {
			fmt.Printf("  [Error] Navigation failed: %v\n", err)
			continue
		}
		
		// Wait for stability
		s.WaitForStability(5 * time.Second)
		navTime := time.Since(start)
		fmt.Printf("  [Success] Loaded in %v\n", navTime)
		
		// Test Perception
		pStart := time.Now()
		aom, err := s.GetAom(browser.AomConfig{WithSpatial: true})
		if err != nil {
			fmt.Printf("  [Error] Perception failed: %v\n", err)
			continue
		}
		pTime := time.Since(pStart)
		
		fmt.Printf("  [Success] Perception took %v\n", pTime)
		fmt.Printf("  [AOM Size] %d nodes (approx)\n", len(aom)/100) // Rough node count
		
		// Take a quick screenshot to verify render
		filename := fmt.Sprintf("real_world_%s.png", time.Now().Format("150405"))
		if err := s.TakeScreenshot(filename); err == nil {
			fmt.Printf("  [Screenshot] Saved to %s\n", filename)
		}
	}
}
