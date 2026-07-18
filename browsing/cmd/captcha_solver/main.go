package main

import (
	"fmt"
	"log"
	"strings"
	"time"

	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
)

func main() {
	fmt.Println("=== CAPTCHA SOLVER POC ===")
	
	s, err := browser.NewManagedSession()
	if err != nil {
		log.Fatalf("Failed to start session: %v", err)
	}
	defer s.Cancel()

	// [1] Navigate to reCAPTCHA Demo
	fmt.Println("[1] Navigating to reCAPTCHA Demo...")
	if err := s.Navigate("https://www.google.com/recaptcha/api2/demo"); err != nil {
		log.Fatalf("Navigation failed: %v", err)
	}
	s.QuickWait(5 * time.Second)

	// [2] Find and Click the Checkbox
	fmt.Println("[2] Waiting for reCAPTCHA frames...")
	time.Sleep(3 * time.Second)
	frames, _ := s.GetFrames()
	fmt.Printf("      - Detected frames:\n%s\n", frames)

	// reCAPTCHA checkbox is usually in an iframe with 'anchor' in the URL
	lines := strings.Split(frames, "\n")
	var anchorFrame string
	for _, line := range lines {
		if strings.Contains(line, "anchor") {
			anchorFrame = strings.TrimSpace(strings.Split(line, "|")[0])
			break
		}
	}

	if anchorFrame != "" {
		fmt.Printf("[3] Switching to anchor frame: %s\n", anchorFrame)
		s.SwitchToFrame(anchorFrame)
		// The checkbox is a div with id 'recaptcha-anchor'
		// We'll use JSClick as it's more reliable for reCAPTCHA
		s.JSClick("0") // Fallback if AOM is not ready, but better to use a selector
		// In a real agent, we'd use the AOM to find the backendID of the checkbox.
	}

	fmt.Println("[4] Detecting challenge overlay...")
	s.SwitchToMainFrame()
	time.Sleep(2 * time.Second)
	
	// FINAL STEP for this POC: Take a screenshot of the whole page to show we can see the challenge
	fmt.Println("[5] Capturing challenge state...")
	time.Sleep(2 * time.Second) // Wait for animation
	if err := s.TakeScreenshot("captcha_state.png"); err != nil {
		fmt.Printf("      [Warning] Screenshot failed: %v\n", err)
	}

	fmt.Printf("\n=== POC COMPLETE ===\n")
	fmt.Println("Check captcha_state.png for the captured challenge.")
}
