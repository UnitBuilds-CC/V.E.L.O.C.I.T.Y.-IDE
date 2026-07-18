package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"os"
	"sync"

	"github.com/reclamation-admin/agentic-browser-go/pkg/browser"
	"github.com/chromedp/chromedp"
)

type PageData struct {
	URL      string   `json:"url"`
	Title    string   `json:"title"`
	Text     string   `json:"text"`
	Links    []string `json:"links"`
	Scripts  []string `json:"scripts"`
	Cookies  []string `json:"cookies"`
	Error    string   `json:"error,omitempty"`
}

func main() {
	baseURL := flag.String("url", "", "Base URL to crawl")
	concurrency := flag.Int("concurrency", 2, "Number of parallel instances")
	flag.Parse()

	if *baseURL == "" {
		log.Fatal("Usage: crawler_graph --url <url> [--concurrency <n>]")
	}

	// 1. Crawler State
	queue := make(chan string, 100)
	results := make(chan PageData, 100)
	var wg sync.WaitGroup

	// 2. Worker Pool (Spawning Instances)
	for i := 0; i < *concurrency; i++ {
		go func() {
			for target := range queue {
				fmt.Fprintf(os.Stderr, "[Worker] Auditing: %s\n", target)
				data := auditPage(target)
				results <- data
				wg.Done()
			}
		}()
	}

	// 3. Start Audit
	wg.Add(1)
	queue <- *baseURL

	// 4. Result Processing (Wait and close)
	go func() {
		wg.Wait()
		close(queue)
		close(results)
	}()

	// 5. Output JSON Stream
	fmt.Println("[")
	first := true
	for res := range results {
		if !first {
			fmt.Println(",")
		}
		jsonOutput, _ := json.MarshalIndent(res, "  ", "  ")
		fmt.Print(string(jsonOutput))
		first = false
	}
	fmt.Println("\n]")
}

func auditPage(target string) PageData {
	sess, err := browser.NewManagedSession()
	if err != nil {
		return PageData{URL: target, Error: err.Error()}
	}
	defer sess.Close()

	if err := sess.Navigate(target); err != nil {
		return PageData{URL: target, Error: err.Error()}
	}

	sess.WaitForStability(3000)

	var title string
	_ = chromedp.Run(sess.Ctx, chromedp.Evaluate("document.title", &title))

	_, _ = sess.GetAom(browser.AomConfig{})

	return PageData{
		URL:     target,
		Title:   title,
		Text:    sess.GetPageText(),
		Links:   sess.GetLinks(),
		Scripts: sess.GetScripts(),
		Cookies: sess.GetCookies(),
	}
}
