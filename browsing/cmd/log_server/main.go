package main

import (
	"fmt"
	"log"
	"net/http"
	"sort"
)

func main() {
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		log.Printf("--- Incoming Request from %s ---", r.RemoteAddr)
		log.Printf("Method: %s", r.Method)
		log.Printf("URL: %s", r.URL)
		log.Printf("Proto: %s", r.Proto)

		// Get headers in sorted order to inspect consistency
		keys := make([]string, 0, len(r.Header))
		for k := range r.Header {
			keys = append(keys, k)
		}
		sort.Strings(keys)

		fmt.Fprintf(w, "<html><body style='font-family: monospace; background: #111; color: #eee; padding: 20px;'>")
		fmt.Fprintf(w, "<h1>Packet Identity Audit</h1>")
		fmt.Fprintf(w, "<h3>Headers:</h3><ul>")
		
		for _, k := range keys {
			val := r.Header.Get(k)
			log.Printf("  %s: %s", k, val)
			fmt.Fprintf(w, "<li><b>%s:</b> %s</li>", k, val)
		}
		fmt.Fprintf(w, "</ul>")
		
		// Echo back the body if any
		fmt.Fprintf(w, "</body></html>")
	})

	port := ":8888"
	fmt.Printf("Identity Probe Server listening on http://localhost%s\n", port)
	if err := http.ListenAndServe(port, nil); err != nil {
		log.Fatal(err)
	}
}
