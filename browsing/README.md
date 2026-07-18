# Agentic Browser (Go Edition)

This is a port of the Agentic Browser engine to Go, tailored for Google-centric ecosystems and high-performance concurrent automation.

## Architecture
- **`pkg/browser`**: Core logic for AOM pruning, serialization, and browser interaction using `chromedp`.
- **`cmd/crawler`**: Multi-step BFS crawler using goroutines for high-throughput exploration.
- **`cmd/api`**: REST API built with `gin` for pathfinding and RPA studio integration.

## Why Go?
1. **Concurrency**: Native goroutines allow for massively parallel crawling without the overhead of .NET async tasks.
2. **Performance**: Minimal startup time and lower memory footprint for containerized deployment.
3. **Ecosystem**: Direct alignment with Google cloud and engineering standards.

## How to Run
1. Ensure Go 1.24+ is installed.
2. Start Chrome with `--remote-debugging-port=9222`.
3. Run the crawler:
   ```bash
   go run cmd/crawler/main.go
   ```
4. Start the API:
   ```bash
   go run cmd/api/main.go
   ```
