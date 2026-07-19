package main

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"os"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/reclamation-admin/agentic-browser-go/pkg/db"
)

type GenerateRequest struct {
	StartURL    string `json:"startUrl"`
	TargetLabel string `json:"targetLabel"`
}

func main() {
	fmt.Println("Go Agentic API starting (SiteMap/NDA Mode)...")

	// 1. Connect to local SiteMap database
	client, err := db.NewClient()
	if err != nil {
		log.Fatalf("Failed to open SiteMap database: %v", err)
	}
	defer client.Close(context.Background())

	// 2. Setup Gin
	r := gin.Default()

	// 3. CORS Middleware
	r.Use(func(c *gin.Context) {
		c.Writer.Header().Set("Access-Control-Allow-Origin", "*")
		c.Writer.Header().Set("Access-Control-Allow-Credentials", "true")
		c.Writer.Header().Set("Access-Control-Allow-Headers", "Content-Type, Content-Length, Accept-Encoding, X-CSRF-Token, Authorization, accept, origin, Cache-Control, X-Requested-With")
		c.Writer.Header().Set("Access-Control-Allow-Methods", "POST, OPTIONS, GET, PUT")

		if c.Request.Method == "OPTIONS" {
			c.AbortWithStatus(204)
			return
		}
		c.Next()
	})

	// 4. Graph Data Endpoint (for Neovis configuration)
	r.GET("/api/graph/config", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"serverUrl": "sitemap_db",
			"user":      "",
			"pass":      "",
		})
	})

	r.POST("/api/graph/wipe", func(c *gin.Context) {
		// Wipe local sitemap directory contents
		_ = os.RemoveAll("sitemap_db")
		_ = os.MkdirAll("sitemap_db/nodes", 0755)
		c.JSON(http.StatusOK, gin.H{"status": "wiped"})
	})

	// 5. Summary Artifact Server
	// Serve summaries from the brain directory
	summaryDir := "./data/summaries"
	os.MkdirAll(summaryDir, 0755)
	r.StaticFS("/api/summaries", http.Dir(summaryDir))

	r.POST("/api/flows/generate", func(c *gin.Context) {
		var req GenerateRequest
		if err := c.ShouldBindJSON(&req); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		script, warnings, err := buildFlowScript(req)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		c.JSON(http.StatusOK, gin.H{
			"script":   script,
			"strategy": "label-driven",
			"warnings": warnings,
		})
	})

	r.GET("/", func(c *gin.Context) {
		c.Data(http.StatusOK, "text/html; charset=utf-8", []byte("<h1>Go Agentic RPA Studio</h1>"))
	})

	fmt.Println("API listening on :8080")
	r.Run(":8080")
}

func buildFlowScript(req GenerateRequest) (string, []string, error) {
	startURL := strings.TrimSpace(req.StartURL)
	targetLabel := strings.TrimSpace(req.TargetLabel)
	if startURL == "" {
		return "", nil, fmt.Errorf("startUrl is required")
	}
	if _, err := url.ParseRequestURI(startURL); err != nil {
		return "", nil, fmt.Errorf("startUrl must be a valid absolute URL: %w", err)
	}
	if targetLabel == "" {
		return "", nil, fmt.Errorf("targetLabel is required")
	}

	warnings := make([]string, 0, 1)
	if len(targetLabel) < 3 {
		warnings = append(warnings, "Very short targetLabel values may match multiple elements.")
	}

	script := fmt.Sprintf(`const flow = {
  startUrl: %q,
  strategy: "label-driven",
  steps: [
    { action: "navigate", url: %q },
    { action: "waitForText", text: %q, timeoutMs: 10000 },
    { action: "clickText", text: %q },
    { action: "assertVisible", text: %q }
  ]
};

export default flow;
`, startURL, startURL, targetLabel, targetLabel, targetLabel)

	return script, warnings, nil
}
