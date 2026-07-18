package main

import (
	"database/sql"
	"fmt"
	"log"
	"os"
	"path/filepath"

	_ "github.com/mattn/go-sqlite3"
)

func main() {
	if len(os.Args) < 2 {
		log.Fatal("Usage: age_profile <profile_path>")
	}
	profilePath := os.Args[1]
	historyPath := filepath.Join(profilePath, "Default", "History")

	fmt.Printf("[Veteran] Aging profile at %s by 30 days...\n", profilePath)

	// 30 days in microseconds
	ageDelta := int64(30 * 24 * 60 * 60 * 1000000)

	// 1. Age History
	db, err := sql.Open("sqlite3", historyPath)
	if err == nil {
		defer db.Close()
		_, err = db.Exec("UPDATE visits SET visit_time = visit_time - ?", ageDelta)
		if err != nil { log.Printf("History age failed: %v", err) }
		_, err = db.Exec("UPDATE urls SET last_visit_time = last_visit_time - ?", ageDelta)
		if err != nil { log.Printf("URLs age failed: %v", err) }
		fmt.Println("  [+] History Aged.")
	} else {
		log.Printf("Could not open History: %v", err)
	}

	// 2. Age Cookies
	possibleCookiePaths := []string{
		filepath.Join(profilePath, "Default", "Network", "Cookies"),
		filepath.Join(profilePath, "Default", "Cookies"),
	}

	var cookiesPath string
	for _, p := range possibleCookiePaths {
		if _, err := os.Stat(p); err == nil {
			cookiesPath = p
			break
		}
	}

	if cookiesPath != "" {
		db2, err := sql.Open("sqlite3", cookiesPath)
		if err == nil {
			defer db2.Close()
			_, err = db2.Exec("UPDATE cookies SET creation_utc = creation_utc - ?", ageDelta)
			if err != nil {
				log.Printf("Cookie age failed: %v", err)
			} else {
				fmt.Printf("  [+] Cookies Aged (%s).\n", filepath.Base(cookiesPath))
			}
		} else {
			log.Printf("Could not open Cookies: %v", err)
		}
	} else {
		log.Println("  [-] No Cookies database found to age.")
	}

	fmt.Println("[Veteran] Profile Aging Complete.")
}
