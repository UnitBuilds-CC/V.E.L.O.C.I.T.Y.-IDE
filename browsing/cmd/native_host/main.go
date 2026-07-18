package main

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"os/exec"
	"runtime"
	"time"
)

func main() {
	logPath := "/app/native_host.log"
	f, _ := os.OpenFile(logPath, os.O_RDWR|os.O_CREATE|os.O_TRUNC, 0666)
	log.SetOutput(f)
	log.SetFlags(log.LstdFlags | log.Lshortfile)
	log.Println("[Relay] Native Host Starting...")
	
	// Connect to the Go orchestrator via TCP or Unix Socket (with retry)
	var conn net.Conn
	var err error
	for i := 0; i < 10; i++ {
		if runtime.GOOS == "linux" {
			conn, err = net.Dial("unix", "/tmp/ghost.sock")
		} else {
			conn, err = net.Dial("tcp", "127.0.0.1:9998")
		}
		if err == nil {
			break
		}
		log.Printf("Dial failed (attempt %d): %v. Retrying...", i+1, err)
		time.Sleep(1 * time.Second)
	}
	if err != nil {
		log.Fatalf("Failed to connect to main app after retries: %v", err)
	}
	defer conn.Close()

	// Relay messages from Chrome (Stdin) to Main App (Socket)
	go func() {
		for {
			var length uint32
			if err := binary.Read(os.Stdin, binary.LittleEndian, &length); err != nil {
				if err == io.EOF { break }
				log.Printf("Error reading length from Stdin: %v", err)
				return
			}

			payload := make([]byte, length)
			if _, err := io.ReadFull(os.Stdin, payload); err != nil {
				log.Printf("Error reading payload from Stdin: %v", err)
				return
			}
			log.Printf("Received from Chrome: %s", string(payload))
			conn.Write(append(payload, '\n')) // Add newline for socket framing
		}
	}()

	// Relay messages from Main App (Socket) to Chrome (Stdout) OR handle locally
	decoder := json.NewDecoder(conn)
	for {
		var msg map[string]interface{}
		if err := decoder.Decode(&msg); err != nil {
			if err == io.EOF { break }
			log.Printf("Error decoding from Socket: %v", err)
			return
		}

		// Check for OS-level automation commands
		msgType, _ := msg["type"].(string)
		switch msgType {
		case "CLICK", "MOVE", "MOVE_SMOOTH", "MOUSE_DOWN", "MOUSE_UP":
			x := int(msg["x"].(float64))
			y := int(msg["y"].(float64))
			log.Printf("Performing OS-Level %s at (%d, %d)", msgType, x, y)

			switch runtime.GOOS {
			case "linux":
				switch msgType {
				case "CLICK":
					exec.Command("xdotool", "mousemove", fmt.Sprint(x), fmt.Sprint(y), "click", "1").Run()
				case "MOVE":
					exec.Command("xdotool", "mousemove", fmt.Sprint(x), fmt.Sprint(y)).Run()
				case "MOVE_SMOOTH":
					exec.Command("xdotool", "mousemove", "--sync", fmt.Sprint(x), fmt.Sprint(y)).Run()
				case "MOUSE_DOWN":
					exec.Command("xdotool", "mousemove", fmt.Sprint(x), fmt.Sprint(y), "mousedown", "1").Run()
				case "MOUSE_UP":
					exec.Command("xdotool", "mousemove", fmt.Sprint(x), fmt.Sprint(y), "mouseup", "1").Run()
				}
			case "windows":
				// Note: Could add Windows mouse simulation here if needed
				log.Println("OS-Level simulation not implemented on Windows yet")
			}
			continue // Don't relay OS-level commands to Chrome
		}

		payload, _ := json.Marshal(msg)
		log.Printf("Relaying to Chrome: %s", string(payload))
		
		binary.Write(os.Stdout, binary.LittleEndian, uint32(len(payload)))
		os.Stdout.Write(payload)
		os.Stdout.Sync()
	}
}
