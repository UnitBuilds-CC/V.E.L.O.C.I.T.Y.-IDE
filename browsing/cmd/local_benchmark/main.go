package main

import (
	"fmt"
	"log"
	"time"

	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

func main() {
	fmt.Println("=== LOCAL REACT APP BENCHMARK ===")
	
	s, err := browser.NewManagedSession()
	if err != nil {
		log.Fatalf("Failed to start session: %v", err)
	}
	defer s.Cancel()

	start := time.Now()

	// [1] Navigate to Local App
	fmt.Println("[1] Navigating to Local RealWorld App...")
	if err := s.Navigate("http://localhost:3000"); err != nil {
		log.Fatalf("Navigation failed: %v", err)
	}
	s.QuickWait(5 * time.Second)

	// [2] Go to Sign In
	fmt.Println("[2] Navigating to Sign In...")
	aom, _ := s.GetAom(browser.AomConfig{})
	_ = aom // for now
	
	if err := s.ClickByName("Sign in"); err != nil {
		fmt.Printf("      [Warning] Sign in button click failed: %v\n", err)
	}
	s.QuickWait(2 * time.Second)

	// [3] Fill Login Form
	fmt.Println("[3] Filling Login Form...")
	if err := s.TypeTextByPlaceholder("Email", "test@example.com"); err != nil {
		fmt.Printf("      [Warning] Email input failed: %v\n", err)
	}
	if err := s.TypeTextByPlaceholder("Password", "password"); err != nil {
		fmt.Printf("      [Warning] Password input failed: %v\n", err)
	}

	// [4] Final Perception
	fmt.Println("[4] Final Perception...")
	aom, err = s.GetAom(browser.AomConfig{})
	if err != nil {
		log.Fatalf("Final perception failed: %v", err)
	}
	
	fmt.Printf("      - Final AOM Length: %d chars\n", len(aom))

	fmt.Printf("\n=== BENCHMARK COMPLETE ===\n")
	fmt.Printf("Total Time: %v\n", time.Since(start))
}
