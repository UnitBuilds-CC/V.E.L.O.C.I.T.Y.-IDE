package main

import (
	"context"
	"flag"
	"log"
	"os"
	"strings"

	"github.com/reclamation-admin/agentic-browser-go/pkg/graph"
)

func main() {
	url := flag.String("url", "", "Page URL")
	title := flag.String("title", "", "Page Title")
	summary := flag.String("summary", "", "Page Summary Preview")
	artifact := flag.String("artifact", "", "Artifact Path")
	links := flag.String("links", "", "Comma-separated list of outbound links")
	scripts := flag.String("scripts", "", "Comma-separated list of script domains")
	cookies := flag.String("cookies", "", "Comma-separated list of cookie identifiers")
	flag.Parse()

	if *url == "" {
		log.Fatal("Usage: graph_upsert --url <url> --title <title> --summary <summary> --artifact <path> [--links <l1,l2>] [--scripts <s1,s2>] [--cookies <c1,c2>]")
	}

	// 1. Setup Neo4j
	uri := os.Getenv("NEO4J_URI")
	if uri == "" { uri = "bolt://localhost:7687" }
	user := os.Getenv("NEO4J_USER")
	if user == "" { user = "neo4j" }
	pass := os.Getenv("NEO4J_PASS")
	if pass == "" { pass = "agentic_secure_password" }

	driver, err := graph.NewDriver(uri, user, pass)
	if err != nil {
		log.Fatalf("Failed to connect to Neo4j: %v", err)
	}
	defer driver.Close()

	ctx := context.Background()

	// 2. Ensure Index
	if err := driver.EnsureFullTextIndex(ctx); err != nil {
		log.Printf("[Warning] Failed to ensure full-text index: %v", err)
	}

	// 3. Prepare Lists
	linkList := splitComma(*links)
	scriptList := splitComma(*scripts)
	cookieList := splitComma(*cookies)

	// 4. Atomic Upsert
	if err := driver.UpsertPage(ctx, *url, *title, *summary, *artifact, linkList, scriptList, cookieList); err != nil {
		log.Fatalf("Failed to upsert page and infrastructure: %v", err)
	}

	log.Printf("[GraphUpsert] Successfully persisted %s and its infrastructure\n", *url)
}

func splitComma(s string) []string {
	parts := []string{}
	if s != "" {
		for _, p := range strings.Split(s, ",") {
			trimmed := strings.TrimSpace(p)
			if trimmed != "" {
				parts = append(parts, trimmed)
			}
		}
	}
	return parts
}
