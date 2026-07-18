package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"os/exec"
	"time"
)

func main() {
	fmt.Println("Starting MCP Client Test...")

	// 1. Start the MCP Server process
	cmd := exec.Command("./agentic-browser-mcp.exe")
	
	stdin, err := cmd.StdinPipe()
	if err != nil {
		log.Fatalf("Failed to get stdin pipe: %v", err)
	}
	
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		log.Fatalf("Failed to get stdout pipe: %v", err)
	}

	if err := cmd.Start(); err != nil {
		log.Fatalf("Failed to start server: %v", err)
	}
	defer cmd.Process.Kill()

	reader := bufio.NewReader(stdout)

	// Helper to send a request and read the response
	sendRequest := func(req map[string]interface{}) string {
		reqBytes, _ := json.Marshal(req)
		reqBytes = append(reqBytes, '\n')
		
		fmt.Printf(">> Sending: %s", reqBytes)
		stdin.Write(reqBytes)

		// Read response
		respBytes, err := reader.ReadBytes('\n')
		if err != nil {
			if err == io.EOF {
				log.Fatalf("Server closed connection unexpectedly")
			}
			log.Fatalf("Failed to read response: %v", err)
		}
		
		fmt.Printf("<< Received: %s\n", respBytes)
		return string(respBytes)
	}

	// 2. Send Initialize
	sendRequest(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      1,
		"method":  "initialize",
		"params":  map[string]interface{}{},
	})

	// 3. Send Navigate Tool Call
	sendRequest(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      2,
		"method":  "tools/call",
		"params": map[string]interface{}{
			"name": "navigate",
			"arguments": map[string]interface{}{
				"url": "https://www.amazon.com/s?k=bicycle",
			},
		},
	})
	
	// Give the browser a moment to close cleanly if needed
	time.Sleep(1 * time.Second)
	fmt.Println("MCP Client Test Complete.")
}
