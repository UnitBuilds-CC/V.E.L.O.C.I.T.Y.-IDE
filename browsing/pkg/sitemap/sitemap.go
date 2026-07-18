package sitemap

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"
)

// Predicate IDs
const (
	PredicateLinksTo      uint16 = 1
	PredicateUsesScript   uint16 = 2
	PredicateHostedOn     uint16 = 3
	PredicateUsesCookie   uint16 = 4
	PredicateHasChild     uint16 = 5
	PredicateHasAomRoot   uint16 = 6

	PredicateURL          uint16 = 10
	PredicateTitle        uint16 = 11
	PredicateSummary      uint16 = 12
	PredicateArtifactPath uint16 = 13
	PredicateNodeID       uint16 = 14
	PredicateBackendID    uint16 = 15
	PredicateRole         uint16 = 16
	PredicateName         uint16 = 17
	PredicateValue        uint16 = 18
	PredicateIsOffscreen  uint16 = 19
	PredicateStyle        uint16 = 20
)

// NdaNode is the interface representing a Merkle AST node in our system.
type NdaNode interface {
	Hash() uint64
	Write(buf *bytes.Buffer)
}

// TripleNode represents a semantic triple (subject, predicate, object) in the SiteMap.
type TripleNode struct {
	SubjectHash uint64
	PredicateID uint16
	ObjectHash  uint64
}

func (t *TripleNode) Hash() uint64 {
	h := sha256.New()
	h.Write([]byte("TPL"))
	
	var sb [8]byte
	binary.LittleEndian.PutUint64(sb[:], t.SubjectHash)
	h.Write(sb[:])
	
	var pb [2]byte
	binary.LittleEndian.PutUint16(pb[:], t.PredicateID)
	h.Write(pb[:])
	
	var ob [8]byte
	binary.LittleEndian.PutUint64(ob[:], t.ObjectHash)
	h.Write(ob[:])
	
	digest := h.Sum(nil)
	return binary.LittleEndian.Uint64(digest[:8])
}

func (t *TripleNode) Write(buf *bytes.Buffer) {
	buf.WriteByte('T')
	
	var sb [8]byte
	binary.LittleEndian.PutUint64(sb[:], t.SubjectHash)
	buf.Write(sb[:])
	
	var pb [2]byte
	binary.LittleEndian.PutUint16(pb[:], t.PredicateID)
	buf.Write(pb[:])
	
	var ob [8]byte
	binary.LittleEndian.PutUint64(ob[:], t.ObjectHash)
	buf.Write(ob[:])
}

// ScopeNode represents a container/collection of child nodes.
type ScopeNode struct {
	Children []NdaNode
}

func (s *ScopeNode) Hash() uint64 {
	h := sha256.New()
	h.Write([]byte("SCP"))
	for _, child := range s.Children {
		var cb [8]byte
		binary.LittleEndian.PutUint64(cb[:], child.Hash())
		h.Write(cb[:])
	}
	digest := h.Sum(nil)
	return binary.LittleEndian.Uint64(digest[:8])
}

func (s *ScopeNode) Write(buf *bytes.Buffer) {
	buf.WriteByte('S')
	var lenBytes [4]byte
	binary.LittleEndian.PutUint32(lenBytes[:], uint32(len(s.Children)))
	buf.Write(lenBytes[:])
	for _, child := range s.Children {
		child.Write(buf)
	}
}

// IntNode represents an integer value.
type IntNode struct {
	Value int32
}

func (i *IntNode) Hash() uint64 {
	h := sha256.New()
	h.Write([]byte("INT"))
	var vb [4]byte
	binary.LittleEndian.PutUint32(vb[:], uint32(i.Value))
	h.Write(vb[:])
	digest := h.Sum(nil)
	return binary.LittleEndian.Uint64(digest[:8])
}

func (i *IntNode) Write(buf *bytes.Buffer) {
	buf.WriteByte('I')
	var vb [4]byte
	binary.LittleEndian.PutUint32(vb[:], uint32(i.Value))
	buf.Write(vb[:])
}

// SiteMap manages our Merkle graph files on disk.
type SiteMap struct {
	basePath string
	mu       sync.Mutex
	dict     map[uint64]string
}

// Open initializes or opens a SiteMap workspace.
func Open(basePath string) (*SiteMap, error) {
	if err := os.MkdirAll(filepath.Join(basePath, "nodes"), 0755); err != nil {
		return nil, err
	}
	sm := &SiteMap{
		basePath: basePath,
		dict:     make(map[uint64]string),
	}
	_ = sm.loadDict()
	return sm, nil
}

// HashString returns a u64 hash of a string using SHA256.
func HashString(s string) uint64 {
	h := sha256.New()
	h.Write([]byte(s))
	digest := h.Sum(nil)
	return binary.LittleEndian.Uint64(digest[:8])
}

// SaveNode writes a node to the content-addressed nodes folder.
func (sm *SiteMap) SaveNode(node NdaNode) (uint64, error) {
	sm.mu.Lock()
	defer sm.mu.Unlock()

	hash := node.Hash()
	filename := filepath.Join(sm.basePath, "nodes", fmt.Sprintf("%016x.nda", hash))
	
	// Avoid redundant writes
	if _, err := os.Stat(filename); err == nil {
		return hash, nil
	}

	var buf bytes.Buffer
	node.Write(&buf)

	if err := os.WriteFile(filename, buf.Bytes(), 0644); err != nil {
		return 0, err
	}

	return hash, nil
}

// RegisterString records the mapping of a u64 hash to its raw string value.
func (sm *SiteMap) RegisterString(s string) uint64 {
	sm.mu.Lock()
	defer sm.mu.Unlock()

	h := HashString(s)
	if _, ok := sm.dict[h]; !ok {
		sm.dict[h] = s
		_ = sm.saveDict()
	}
	return h
}

// ResolveString returns the raw string value of a hash, if registered.
func (sm *SiteMap) ResolveString(h uint64) (string, bool) {
	sm.mu.Lock()
	defer sm.mu.Unlock()
	val, ok := sm.dict[h]
	return val, ok
}

func (sm *SiteMap) loadDict() error {
	path := filepath.Join(sm.basePath, "dictionary.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var raw map[string]string
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}
	for kStr, v := range raw {
		var k uint64
		if _, err := fmt.Sscanf(kStr, "%x", &k); err == nil {
			sm.dict[k] = v
		}
	}
	return nil
}

func (sm *SiteMap) saveDict() error {
	path := filepath.Join(sm.basePath, "dictionary.json")
	raw := make(map[string]string)
	for k, v := range sm.dict {
		raw[fmt.Sprintf("%016x", k)] = v
	}
	data, err := json.MarshalIndent(raw, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(path, data, 0644)
}

// BasePath returns the root directory of the SiteMap database.
func (sm *SiteMap) BasePath() string {
	return sm.basePath
}

