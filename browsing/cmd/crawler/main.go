package main

import (
	"context"
	"fmt"
	"hash/maphash"
	"log"
	"os"
	"strings"
	"sync"

	"github.com/chromedp/chromedp"
	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
	"github.com/reclamation-admin/agentic-browser-go/pkg/sitemap"
)

type ActionStep struct {
	Role  string
	Name  string
	Index int // Handle duplicate names/roles
}

type ActionPath struct {
	Steps []ActionStep
}

type Crawler struct {
	sm       *sitemap.SiteMap
	visited  map[string]bool
	queue    []ActionPath
	maxDepth int
	startURL string
	mu       sync.Mutex
}

func main() {
	fmt.Println("Go Concurrent BFS Crawler (Robust Engine) starting (SiteMap/NDA Mode)...")

	// 1. Setup local SiteMap database
	uri := os.Getenv("NEO4J_URI")
	if uri == "" || stringsContainsNeo4j(uri) {
		uri = "sitemap_db"
	}
	sm, err := sitemap.Open(uri)
	if err != nil {
		log.Fatalf("Failed to initialize SiteMap database: %v", err)
	}

	// 2. Start Global Allocator
	allocCtx, cancel := chromedp.NewExecAllocator(context.Background(),
		append(chromedp.DefaultExecAllocatorOptions[:],
			chromedp.NoSandbox,
			chromedp.Flag("disable-setuid-sandbox", true),
			chromedp.Flag("headless", "new"),
		)...)
	defer cancel()

	// 3. Initialize Crawler State
	maxDepth := 3
	if d := os.Getenv("CRAWLER_DEPTH"); d != "" {
		fmt.Sscanf(d, "%d", &maxDepth)
	}
	startURL := os.Getenv("CRAWLER_START_URL")
	if startURL == "" {
		startURL = "http://localhost:8081/index.html"
	}

	crawler := &Crawler{
		sm:       sm,
		visited:  make(map[string]bool),
		queue:    []ActionPath{{Steps: []ActionStep{}}},
		maxDepth: maxDepth,
		startURL: startURL,
	}

	// 4. Run Workers
	numWorkers := 3
	if w := os.Getenv("CRAWLER_WORKERS"); w != "" {
		fmt.Sscanf(w, "%d", &numWorkers)
	}
	var wg sync.WaitGroup
	for i := 0; i < numWorkers; i++ {
		wg.Add(1)
		go func(id int) {
			defer wg.Done()
			crawler.runWorker(id, allocCtx)
		}(i)
	}

	wg.Wait()
	fmt.Println("\nGo Concurrent BFS Crawler finished.")
}

func stringsContainsNeo4j(s string) bool {
	return len(s) > 4 && (s[:4] == "bolt" || s[:4] == "neo4" || s[:4] == "http")
}

func (c *Crawler) runWorker(id int, allocCtx context.Context) {
	fmt.Printf("[Worker %d] Started.\n", id)
	
	// Each worker gets its own browser context from the global allocator
	ctx, cancel := chromedp.NewContext(allocCtx)
	defer cancel()

	sess := &browser.Session{Ctx: ctx}
	var h maphash.Hash

	for {
		c.mu.Lock()
		if len(c.queue) == 0 {
			c.mu.Unlock()
			return
		}
		currentPath := c.queue[0]
		c.queue = c.queue[1:]
		c.mu.Unlock()

		if len(currentPath.Steps) >= c.maxDepth {
			continue
		}

		fmt.Printf("[Worker %d] Exploring depth %d...\n", id, len(currentPath.Steps))

		// Reset and Replay
		if err := replayPath(sess, c.startURL, currentPath); err != nil {
			fmt.Printf("[Worker %d] Error replaying: %v\n", id, err)
			continue
		}

		// Map Current State
		_, err := sess.GetAom(browser.AomConfig{})
		if err != nil {
			fmt.Printf("[Worker %d] Error getting AOM: %v\n", id, err)
			continue
		}

		stateHash := hashStructuralState(&h, sess.LastAom)
		
		c.mu.Lock()
		if c.visited[stateHash] {
			c.mu.Unlock()
			continue
		}
		c.visited[stateHash] = true
		c.mu.Unlock()

		fmt.Printf("[Worker %d] New state: %s\n", id, stateHash)
		mergeState(c.sm, stateHash, c.startURL)

		// Extract and filter
		targets := extractInteractables(sess.LastAom)
		
		// Track indices for identical name/role pairs to ensure unique replay
		typeCount := make(map[string]int)

		for _, node := range targets {
			key := fmt.Sprintf("%s|%s", node.Role, node.Name)
			idx := typeCount[key]
			typeCount[key]++

			if isDestructive(node.Name) {
				continue
			}

			fmt.Printf("[Worker %d]   Trying %s (%s) #%d...\n", id, node.Name, node.Role, idx)

			// Interaction logic
			if node.Role == "textbox" {
				sess.TypeText(node.NodeID, "test_input")
			} else {
				sess.JSClick(node.NodeID)
			}
			sess.WaitForStability(0)

			_, err := sess.GetAom(browser.AomConfig{})
			if err != nil { continue }
			
			newStateHash := hashStructuralState(&h, sess.LastAom)
			mergeActionEdge(c.sm, stateHash, newStateHash, node, idx)

			// Enqueue next path
			nextSteps := make([]ActionStep, len(currentPath.Steps)+1)
			copy(nextSteps, currentPath.Steps)
			nextSteps[len(currentPath.Steps)] = ActionStep{Role: node.Role, Name: node.Name, Index: idx}
			
			c.mu.Lock()
			c.queue = append(c.queue, ActionPath{Steps: nextSteps})
			c.mu.Unlock()

			// Reset back for next sibling in this worker
			replayPath(sess, c.startURL, currentPath)
		}
	}
}

func replayPath(s *browser.Session, startURL string, path ActionPath) error {
	if err := s.Navigate(startURL); err != nil { return err }
	s.WaitForStability(0)
	for _, step := range path.Steps {
		s.GetAom(browser.AomConfig{})
		targets := extractInteractables(s.LastAom)
		
		var target *browser.PrunedNode
		currIdx := 0
		for _, node := range targets {
			if node.Name == step.Name && node.Role == step.Role {
				if currIdx == step.Index {
					target = node
					break
				}
				currIdx++
			}
		}
		
		if target == nil { return fmt.Errorf("node %s (%s) #%d not found", step.Name, step.Role, step.Index) }
		if target.Role == "textbox" { s.TypeText(target.NodeID, "test_input") } else { s.JSClick(target.NodeID) }
		s.WaitForStability(0)
	}
	return nil
}

func hashStructuralState(h *maphash.Hash, nodes []*browser.PrunedNode) string {
	h.Reset()
	var traverse func([]*browser.PrunedNode, int)
	traverse = func(ns []*browser.PrunedNode, depth int) {
		for _, n := range ns {
			h.WriteString(fmt.Sprintf("%d:%s;", depth, n.Role))
			traverse(n.Children, depth+1)
		}
	}
	traverse(nodes, 0)
	return fmt.Sprintf("%016x", h.Sum64())
}

func extractInteractables(nodes []*browser.PrunedNode) []*browser.PrunedNode {
	var res []*browser.PrunedNode
	var find func([]*browser.PrunedNode)
	find = func(ns []*browser.PrunedNode) {
		for _, n := range ns {
			if n.Role == "button" || n.Role == "link" || n.Role == "textbox" {
				res = append(res, n)
			}
			find(n.Children)
		}
	}
	find(nodes)
	return res
}

func isDestructive(name string) bool {
	lowerName := strings.ToLower(name)
	destructiveKeywords := []string{"delete", "remove", "destroy", "logout", "log out", "sign out", "buy", "pay", "checkout", "submit", "confirm", "drop"}
	for _, kw := range destructiveKeywords {
		if strings.Contains(lowerName, kw) { return true }
	}
	return false
}

func mergeState(sm *sitemap.SiteMap, hash, url string) error {
	sub := sm.RegisterString(hash)
	urlHash := sm.RegisterString(url)

	node1 := &sitemap.TripleNode{SubjectHash: sub, PredicateID: sitemap.PredicateURL, ObjectHash: urlHash}
	_, err := sm.SaveNode(node1)
	return err
}

func mergeActionEdge(sm *sitemap.SiteMap, from, to string, node *browser.PrunedNode, index int) error {
	fromHash := sm.RegisterString(from)
	toHash := sm.RegisterString(to)
	nodeNameHash := sm.RegisterString(node.Name)
	nodeRoleHash := sm.RegisterString(node.Role)

	// Save Transition Triples
	node1 := &sitemap.TripleNode{SubjectHash: fromHash, PredicateID: sitemap.PredicateLinksTo, ObjectHash: toHash}
	_, err := sm.SaveNode(node1)

	t1 := &sitemap.TripleNode{SubjectHash: toHash, PredicateID: sitemap.PredicateName, ObjectHash: nodeNameHash}
	c1 := &sitemap.TripleNode{SubjectHash: toHash, PredicateID: sitemap.PredicateRole, ObjectHash: nodeRoleHash}
	_, _ = sm.SaveNode(t1)
	_, _ = sm.SaveNode(c1)

	return err
}
