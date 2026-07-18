package main

import (
	"context"
	"fmt"
	"net"
	"os"
	"strings"
	"time"

	"github.com/reclamation-admin/agentic-browser-go/pkg/swarm"
	"github.com/reclamation-admin/agentic-browser-go/pkg/swarm/proto"
	"github.com/shirou/gopsutil/v3/cpu"
	"google.golang.org/grpc"
)

func main() {
	port := 50051
	lis, err := net.Listen("tcp", fmt.Sprintf(":%d", port))
	if err != nil {
		fmt.Printf("Failed to listen: %v\n", err)
		return
	}
	
	grpcServer := grpc.NewServer()
	agent := swarm.NewNodeAgent()
	proto.RegisterSwarmServiceServer(grpcServer, agent)
	
	// Connect to Orchestrator with Failover support
	endpointsStr := os.Getenv("ORCHESTRATOR_ENDPOINTS")
	if endpointsStr == "" {
		endpointsStr = "localhost:50052" // Default fallback
		endpointsStr = os.Getenv("ORCHESTRATOR_ADDR")
		if endpointsStr == "" {
			endpointsStr = "localhost:50052" // Default fallback
		}
	}
	endpoints := strings.Split(endpointsStr, ",")

	go func() {
		for {
			for _, addr := range endpoints {
				addr = strings.TrimSpace(addr)
				fmt.Printf("[NodeAgent] Attempting to connect to Orchestrator at %s...\n", addr)
				
				conn, err := grpc.Dial(addr, grpc.WithInsecure(), grpc.WithBlock(), grpc.WithTimeout(3*time.Second))
				if err != nil {
					fmt.Printf("[NodeAgent] Failed to connect to %s: %v\n", addr, err)
					continue
				}
				
				client := proto.NewOrchestratorServiceClient(conn)
				agent.SetOrchestrator(client)
				
				// 1. Register Node
				// Use NODE_ENDPOINT env var if set, otherwise fallback to local IP
				nodeEndpoint := os.Getenv("NODE_ENDPOINT")
				if nodeEndpoint == "" {
					nodeEndpoint = "localhost:50051" 
				}
				fmt.Printf("[NodeAgent] Registering as %s\n", nodeEndpoint)
				
				resp, err := client.RegisterNode(context.Background(), &proto.RegisterNodeRequest{
					Endpoint:     nodeEndpoint,
					Tier:         os.Getenv("NODE_TIER"), 
					MaxSimops:    1000,
					LoadedModels: []string{"local-model"},
				})
				if err != nil {
					fmt.Printf("[NodeAgent] Failed to register with %s: %v\n", addr, err)
					conn.Close()
					continue
				}
				fmt.Printf("[NodeAgent] Registered with %s: %s\n", addr, resp.Message)
				
				// 2. Start Heartbeat loop for this orchestrator
				ticker := time.NewTicker(5 * time.Second)
				failed := false
				for range ticker.C {
					percentages, _ := cpu.Percent(time.Second, false)
					var cpuUtil float32
					if len(percentages) > 0 {
						cpuUtil = float32(percentages[0])
					}

					_, err := client.ReportHeartbeat(context.Background(), &proto.HeartbeatRequest{
						Endpoint:       nodeEndpoint,
						CurrentSimops:  agent.GetActiveSimOps(),
						CpuUtilization: cpuUtil,
					})
					if err != nil {
						fmt.Printf("[NodeAgent] Heartbeat to %s failed: %v. Searching for failover...\n", addr, err)
						failed = true
						break
					}
				}
				
				ticker.Stop()
				conn.Close()
				if failed {
					continue // Try next endpoint in the list
				}
			}
			time.Sleep(5 * time.Second) // Wait before restarting the search loop
		}
	}()
	
	fmt.Printf("Node Agent listening on :%d\n", port)
	if err := grpcServer.Serve(lis); err != nil {
		fmt.Printf("Failed to serve: %v\n", err)
	}
}
