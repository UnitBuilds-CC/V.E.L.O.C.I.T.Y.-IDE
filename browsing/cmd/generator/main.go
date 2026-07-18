package main

import (
	"encoding/binary"
	"fmt"
	"log"
	"os"
	"path/filepath"

	"github.com/reclamation-admin/agentic-browser-go/pkg/sitemap"
)

func main() {
	if len(os.Args) < 3 {
		fmt.Println("Usage: generator <start_hash> <end_hash> [sitemap_dir]")
		os.Exit(1)
	}

	startHash := os.Args[1]
	endHash := os.Args[2]

	basePath := "sitemap_db"
	if len(os.Args) >= 4 {
		basePath = os.Args[3]
	}

	// 2. Query Path from SiteMap triples
	steps, startURL, err := queryPathLocal(basePath, startHash, endHash)
	if err != nil {
		log.Fatalf("Failed to query path: %v", err)
	}

	// 3. Generate Go Script
	generateScript(startURL, steps)
}

type ActionStep struct {
	Role  string
	Name  string
	Index int
}

func queryPathLocal(basePath, startHex, endHex string) ([]ActionStep, string, error) {
	sm, err := sitemap.Open(basePath)
	if err != nil {
		return nil, "", err
	}

	startHash := sitemap.HashString(startHex)
	endHash := sitemap.HashString(endHex)

	triples, err := LoadAllTriples(basePath)
	if err != nil {
		return nil, "", err
	}

	// Build adjacency list: fromState -> list of toStates
	adj := make(map[uint64][]uint64)
	for _, t := range triples {
		if t.PredicateID == sitemap.PredicateLinksTo {
			adj[t.SubjectHash] = append(adj[t.SubjectHash], t.ObjectHash)
		}
	}

	// BFS for shortest path
	queue := [][]uint64{{startHash}}
	visited := make(map[uint64]bool)
	visited[startHash] = true

	var path []uint64
	for len(queue) > 0 {
		curr := queue[0]
		queue = queue[1:]

		last := curr[len(curr)-1]
		if last == endHash {
			path = curr
			break
		}

		for _, next := range adj[last] {
			if !visited[next] {
				visited[next] = true
				nextPath := make([]uint64, len(curr)+1)
				copy(nextPath, curr)
				nextPath[len(curr)] = next
				queue = append(queue, nextPath)
			}
		}
	}

	if len(path) == 0 {
		return nil, "", fmt.Errorf("no path found between %s and %s", startHex, endHex)
	}

	// Resolve the start URL
	startURL := "http://localhost:8081/index.html"
	for _, t := range triples {
		if t.SubjectHash == startHash && t.PredicateID == sitemap.PredicateURL {
			if urlVal, ok := sm.ResolveString(t.ObjectHash); ok {
				startURL = urlVal
				break
			}
		}
	}

	// Build action steps from state transitions
	var steps []ActionStep
	for i := 0; i < len(path)-1; i++ {
		toState := path[i+1]
		var name, role string
		for _, t := range triples {
			if t.SubjectHash == toState {
				if t.PredicateID == sitemap.PredicateName {
					if val, ok := sm.ResolveString(t.ObjectHash); ok {
						name = val
					}
				}
				if t.PredicateID == sitemap.PredicateRole {
					if val, ok := sm.ResolveString(t.ObjectHash); ok {
						role = val
					}
				}
			}
		}
		steps = append(steps, ActionStep{
			Role:  role,
			Name:  name,
			Index: 0,
		})
	}

	return steps, startURL, nil
}

func LoadAllTriples(basePath string) ([]sitemap.TripleNode, error) {
	dir := filepath.Join(basePath, "nodes")
	files, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}

	var triples []sitemap.TripleNode
	for _, f := range files {
		if filepath.Ext(f.Name()) != ".nda" {
			continue
		}
		data, err := os.ReadFile(filepath.Join(dir, f.Name()))
		if err != nil {
			continue
		}
		if len(data) == 19 && data[0] == 'T' {
			sub := binary.LittleEndian.Uint64(data[1:9])
			pred := binary.LittleEndian.Uint16(data[9:11])
			obj := binary.LittleEndian.Uint64(data[11:19])
			triples = append(triples, sitemap.TripleNode{
				SubjectHash: sub,
				PredicateID: pred,
				ObjectHash:  obj,
			})
		}
	}
	return triples, nil
}

func generateScript(startURL string, steps []ActionStep) {
	fmt.Println("package main")
	fmt.Println()
	fmt.Println("import (")
	fmt.Println("\t\"context\"")
	fmt.Println("\t\"fmt\"")
	fmt.Println("\t\"log\"")
	fmt.Println("\t\"github.com/chromedp/chromedp\"")
	fmt.Println("\t\"github.com/reclamation-admin/agentic-browser-go/pkg/browser\"")
	fmt.Println(")")
	fmt.Println()
	fmt.Println("func main() {")
	fmt.Println("\tctx, cancel := chromedp.NewContext(context.Background())")
	fmt.Println("\tdefer cancel()")
	fmt.Println()
	fmt.Printf("\tsess := &browser.Session{Ctx: ctx}\n")
	fmt.Printf("\tfmt.Println(\"Starting generated test path...\")\n")
	fmt.Println()
	fmt.Printf("\tlog.Println(\"Navigating to %s\")\n", startURL)
	fmt.Printf("\tif err := sess.Navigate(\"%s\"); err != nil { log.Fatal(err) }\n", startURL)
	fmt.Println("\tsess.WaitForStability(0)")
	fmt.Println()

	for i, step := range steps {
		fmt.Printf("\t// Step %d: %s (%s) #%d\n", i+1, step.Name, step.Role, step.Index)
		fmt.Println("\t{")
		fmt.Println("\t\t_, err := sess.GetAom()")
		fmt.Println("\t\tif err != nil { log.Fatal(err) }")
		fmt.Println("\t\t")
		fmt.Printf("\t\tlog.Println(\"Targeting %s\")\n", step.Name)
		fmt.Printf("\t\tvar targetNodeID string\n")
		fmt.Println("\t\tcurrIdx := 0")
		fmt.Println("\t\tfor _, n := range sess.LastAom {")
		fmt.Printf("\t\t\tif n.Name == %q && n.Role == \"%s\" {\n", step.Name, step.Role)
		fmt.Printf("\t\t\t\tif currIdx == %d {\n", step.Index)
		fmt.Println("\t\t\t\t\ttargetNodeID = n.NodeID")
		fmt.Println("\t\t\t\t\tbreak")
		fmt.Println("\t\t\t\t}")
		fmt.Println("\t\t\t\tcurrIdx++")
		fmt.Println("\t\t\t}")
		fmt.Println("\t\t}")
		fmt.Println("\t\tif targetNodeID == \"\" { log.Fatal(\"Could not find node\") }")
		fmt.Println("\t\tif err := sess.JSClick(targetNodeID); err != nil { log.Fatal(err) }")
		fmt.Println("\t\tsess.WaitForStability(0)")
		fmt.Println("\t}")
		fmt.Println()
	}

	fmt.Println("\tfmt.Println(\"Test path completed successfully!\")")
	fmt.Println("}")
}
