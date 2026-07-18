package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/reclamation-admin/agentic-browser-go/pkg/swarm/proto"
	"google.golang.org/grpc"
)

func main() {
	urlFlag := flag.String("url", "https://www.google.com", "The starting URL for the mission")
	missionFlag := flag.String("mission", "", "The task instructions for the agent")
	addrFlag := flag.String("addr", "", "The Orchestrator address (overrides ORCHESTRATOR_ADDR)")
	flag.Parse()

	url := *urlFlag
	instruction := *missionFlag
	orchestratorAddr := *addrFlag

	if instruction == "" {
		fmt.Println("Usage: mobile_link -mission \"Find something\" [-url \"https://...\"] [-addr \"10.0.0.1:50052\"]")
		return
	}

	if orchestratorAddr == "" {
		orchestratorAddr = os.Getenv("ORCHESTRATOR_ADDR")
		if orchestratorAddr == "" {
			orchestratorAddr = "localhost:50052"
		}
	}

	fmt.Printf("[MobileLink] Connecting to Orchestrator at %s...\n", orchestratorAddr)
	conn, err := grpc.Dial(orchestratorAddr, grpc.WithInsecure(), grpc.WithBlock(), grpc.WithTimeout(5*time.Second))
	if err != nil {
		fmt.Printf("Error: Failed to connect to Orchestrator: %v\n", err)
		return
	}
	defer conn.Close()

	client := proto.NewOrchestratorServiceClient(conn)

	missionId := fmt.Sprintf("mobile_%d", time.Now().Unix())
	
	fmt.Printf("[MobileLink] Submitting mission %s...\n", missionId)
	resp, err := client.SubmitMission(context.Background(), &proto.SubmitMissionRequest{
		Url:         url,
		Instruction: instruction,
		MissionId:   missionId,
	})

	if err != nil {
		fmt.Printf("Error: %v\n", err)
		return
	}

	if resp.Accepted {
		fmt.Printf("SUCCESS: %s\n", resp.Message)
	} else {
		fmt.Printf("FAILED: %s\n", resp.Message)
	}
}
