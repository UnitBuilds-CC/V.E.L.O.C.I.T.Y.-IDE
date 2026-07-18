package browser

import (
	"fmt"
	"strings"
)

// PrunedNode represents a semantic element extracted from the browser's Accessibility Tree.
type PrunedNode struct {
	NodeID      string        `json:"nodeId"`
	BackendID   int64         `json:"backendId"`
	Role        string        `json:"role"`
	Name        string        `json:"name"`
	Value       string        `json:"value"`
	IsOffscreen bool          `json:"isOffscreen"`
	X           int64         `json:"x"`
	Y           int64         `json:"y"`
	W           int64         `json:"w"`
	H           int64         `json:"h"`
	Style       string        `json:"style,omitempty"`
	Attrs       map[string]string `json:"attrs,omitempty"`
	Children    []*PrunedNode `json:"children"`
}

// PruneAom processes the raw AXTree nodes into a compact, LLM-friendly structure.
func PruneAom(nodes []*PrunedNode) []*PrunedNode {
	visited := make(map[int64]bool)
	return pruneRecursive(nodes, visited)
}

func pruneRecursive(nodes []*PrunedNode, visited map[int64]bool) []*PrunedNode {
	var pruned []*PrunedNode
	for _, node := range nodes {
		if node.BackendID != 0 {
			if visited[node.BackendID] {
				continue
			}
			visited[node.BackendID] = true
		}

		if isSemantic(node) {
			node.Children = pruneRecursive(node.Children, visited)

			// Flatten redundant child nodes
			if len(node.Children) == 1 && node.Children[0].Role == "StaticText" && node.Children[0].Name == node.Name {
				node.Children = node.Children[0].Children
			}

			pruned = append(pruned, node)
		} else {
			pruned = append(pruned, pruneRecursive(node.Children, visited)...)
		}
		
		// Safety break to avoid AOM explosion
		if len(visited) > 1000 {
			break
		}
	}
	return pruned
}

var ignoredRoles = map[string]bool{
	"presentation":  true,
	"none":          true,
	"generic":       true,
	"Ignored":       true,
	"InlineTextBox": true,
}

var structuralRoles = map[string]bool{
	"list":        true,
	"listitem":    true,
	"table":       true,
	"row":         true,
	"cell":        true,
	"heading":     true,
	"button":      true,
	"link":        true,
	"searchbox":   true,
	"combobox":    true,
	"textbox":     true,
	"checkbox":    true,
	"radio":       true,
	"RootWebArea": true,
}

func isSemantic(node *PrunedNode) bool {
	if ignoredRoles[node.Role] {
		return false
	}

	// Aggressively prune offscreen elements to save tokens
	// Only keep them if they are structural roles (like button/link) or have content
	if node.IsOffscreen && !structuralRoles[node.Role] {
		return false
	}

	// Keep anything with a Name or Value
	if node.Name != "" || node.Value != "" {
		return true
	}

	return structuralRoles[node.Role]
}

var roleAliases = map[string]string{
	"StaticText":      "text",
	"LayoutTableCell": "cell",
	"RootWebArea":     "root",
	"paragraph":       "p",
}

// AomConfig holds flags for optional sensory layers.
type AomConfig struct {
	WithSpatial    bool
	WithStyles     bool
	WithHeuristics bool
	MaxLength      int
	Summarized     bool
}

// SerializeAom converts the pruned tree into the "ref" string format for LLM consumption.
func SerializeAom(nodes []*PrunedNode, depth int, currentLen *int, cfg AomConfig) string {
	if cfg.MaxLength > 0 && *currentLen >= cfg.MaxLength {
		return ""
	}

	var sb strings.Builder
	for _, node := range nodes {
		if cfg.MaxLength > 0 && *currentLen >= cfg.MaxLength {
			break
		}

		role := strings.ToLower(node.Role)
		isActionable := role == "button" || role == "link" || role == "textbox" || role == "searchbox" || role == "combobox" || role == "image" || role == "heading" || role == "iframe"
		
		if cfg.Summarized && !isActionable {
			// Skip this node but process children with the same depth (flattening)
			childStr := SerializeAom(node.Children, depth, currentLen, cfg)
			sb.WriteString(childStr)
			continue
		}

		indent := strings.Repeat("\t", depth)
		
		var line strings.Builder
		line.WriteString(indent)
		
		if node.IsOffscreen {
			line.WriteString("[offscreen] ")
		}

		role = node.Role
		if alias, ok := roleAliases[role]; ok {
			role = alias
		}

		line.WriteString(fmt.Sprintf("[%s", role))
		
		// Use BackendID for the ref so we can target it reliably in actions
		if node.BackendID != 0 {
			line.WriteString(fmt.Sprintf(" %d", node.BackendID)) // Dropped "ref=" to save tokens
		}
		
		line.WriteString("]")
		
		if node.Name != "" {
			line.WriteString(fmt.Sprintf(" %q", node.Name)) // Using %q wraps in quotes
		}
		
		if node.Value != "" {
			line.WriteString(fmt.Sprintf(" value=%q", node.Value))
		}

		// Optimization: Only include spatial metadata if requested and for actionable or landmark elements.
		role = strings.ToLower(node.Role)
		isActionable = role == "button" || role == "link" || role == "textbox" || role == "searchbox" || role == "combobox" || role == "image" || role == "heading" || role == "iframe"
		if cfg.WithSpatial && node.W > 0 && isActionable {
			line.WriteString(fmt.Sprintf(" @(%d,%d,%d,%d)", node.X, node.Y, node.W, node.H))
		}

		if cfg.WithStyles && node.Style != "" {
			line.WriteString(fmt.Sprintf(" {%s}", node.Style))
		}
		
		line.WriteRune('\n')
		lineStr := line.String()
		sb.WriteString(lineStr)
		*currentLen += len(lineStr)

		if cfg.MaxLength > 0 && *currentLen >= cfg.MaxLength {
			sb.WriteString(indent + "  ... (AOM truncated, too large)\n")
			break
		}

		childStr := SerializeAom(node.Children, depth+1, currentLen, cfg)
		sb.WriteString(childStr)
	}
	return sb.String()
}
