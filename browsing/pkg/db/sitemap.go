package db

import (
	"context"
	"fmt"
	"os"
	"path/filepath"

	"github.com/reclamation-admin/agentic-browser-go/pkg/sitemap"
)

// AOMNode represents a node in the Accessibility Tree for storage.
type AOMNode struct {
	NodeID      string            `json:"nodeId"`
	BackendID   int64             `json:"backendId"`
	Role        string            `json:"role"`
	Name        string            `json:"name"`
	Value       string            `json:"value"`
	IsOffscreen bool              `json:"isOffscreen"`
	X           int64             `json:"x"`
	Y           int64             `json:"y"`
	W           int64             `json:"w"`
	H           int64             `json:"h"`
	Style       string            `json:"style,omitempty"`
	Attrs       map[string]string `json:"attrs,omitempty"`
	Children    []*AOMNode        `json:"children"`
}

// Client handles database operations on the Merkle SiteMap.
type Client struct {
	sm *sitemap.SiteMap
}

// NewClient creates a new Client using the local SiteMap path.
func NewClient() (*Client, error) {
	basePath := os.Getenv("SITEMAP_PATH")
	if basePath == "" {
		if _, err := os.Stat("../.velocity/site_map"); err == nil {
			basePath = "../.velocity/site_map"
		} else {
			basePath = "sitemap_db"
		}
	}
	sm, err := sitemap.Open(basePath)
	if err != nil {
		return nil, err
	}
	return &Client{sm: sm}, nil
}

// Close closes the client.
func (c *Client) Close(ctx context.Context) error {
	return nil
}

// SaveAOM saves the AOM accessibility tree to the SiteMap as structured Merkle triples.
func (c *Client) SaveAOM(ctx context.Context, root *AOMNode, pageURL string) error {
	pageHash := c.sm.RegisterString(pageURL)
	rootHash := c.saveNodeRecursive(root)

	// Link page to AOM root
	c.saveTriple(pageHash, sitemap.PredicateHasAomRoot, rootHash)
	return nil
}

func (c *Client) saveNodeRecursive(node *AOMNode) uint64 {
	nodeIDStr := fmt.Sprintf("AOM_%d_%s", node.BackendID, node.Role)
	nodeHash := c.sm.RegisterString(nodeIDStr)

	roleHash := c.sm.RegisterString(node.Role)
	nameHash := c.sm.RegisterString(node.Name)
	valHash := c.sm.RegisterString(node.Value)
	styleHash := c.sm.RegisterString(node.Style)

	c.saveTriple(nodeHash, sitemap.PredicateRole, roleHash)
	c.saveTriple(nodeHash, sitemap.PredicateName, nameHash)
	c.saveTriple(nodeHash, sitemap.PredicateValue, valHash)
	c.saveTriple(nodeHash, sitemap.PredicateStyle, styleHash)

	// Save positional/backend integers
	backendNode := &sitemap.IntNode{Value: int32(node.BackendID)}
	backHash, _ := c.sm.SaveNode(backendNode)
	c.saveTriple(nodeHash, sitemap.PredicateBackendID, backHash)

	xNode := &sitemap.IntNode{Value: int32(node.X)}
	xHash, _ := c.sm.SaveNode(xNode)
	c.saveTriple(nodeHash, sitemap.PredicateIsOffscreen, xHash) // reuse tag or create custom Int mapping

	// Save children hierarchy
	for _, child := range node.Children {
		childHash := c.saveNodeRecursive(child)
		c.saveTriple(nodeHash, sitemap.PredicateHasChild, childHash)
	}

	return nodeHash
}

func (c *Client) saveTriple(sub uint64, pred uint16, obj uint64) {
	node := &sitemap.TripleNode{
		SubjectHash: sub,
		PredicateID: pred,
		ObjectHash:  obj,
	}
	_, _ = c.sm.SaveNode(node)
}

// SaveSequence saves a named sequence of actions to a local file system inside the SiteMap directory.
func (c *Client) SaveSequence(ctx context.Context, name string, actionsJSON string) error {
	seqPath := filepath.Join(c.sm.BasePath(), "sequences")
	_ = os.MkdirAll(seqPath, 0755)

	filename := filepath.Join(seqPath, name+".json")
	return os.WriteFile(filename, []byte(actionsJSON), 0644)
}

// GetSequence retrieves a named sequence of actions from the sequences directory.
func (c *Client) GetSequence(ctx context.Context, name string) (string, error) {
	filename := filepath.Join(c.sm.BasePath(), "sequences", name+".json")
	data, err := os.ReadFile(filename)
	if err != nil {
		return "", err
	}
	return string(data), nil
}
