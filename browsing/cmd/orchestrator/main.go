package main

import (
	"fmt"
	"net"

	"github.com/reclamation-admin/agentic-browser-go/pkg/sitemap"
	"github.com/reclamation-admin/agentic-browser-go/pkg/swarm"
	"github.com/reclamation-admin/agentic-browser-go/pkg/swarm/proto"
	"google.golang.org/grpc"
)

func main() {
	// Initialize local SiteMap database instead of Neo4j driver
	sm, err := sitemap.Open("sitemap_db")
	if err != nil {
		fmt.Printf("Failed to open SiteMap database: %v\n", err)
		return
	}

	// Create orchestrator with a ledger file and local SiteMap client
	orchestrator := swarm.NewOrchestrator("mission_ledger.jsonl", sm)
	
	// Start Orchestrator gRPC server for registration
	port := 50052
	lis, err := net.Listen("tcp", fmt.Sprintf(":%d", port))
	if err != nil {
		fmt.Printf("Failed to listen on %d: %v\n", port, err)
		return
	}
	
	grpcServer := grpc.NewServer()
	proto.RegisterOrchestratorServiceServer(grpcServer, orchestrator)
	
	fmt.Printf("Orchestrator listening on :%d\n", port)
	go func() {
		if err := grpcServer.Serve(lis); err != nil {
			fmt.Printf("Failed to serve: %v\n", err)
		}
	}()

	fmt.Println("--- Orchestrator running (waiting for nodes and missions) ---")
	select {}
}
