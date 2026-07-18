package graph

import (
	"context"
	"strings"

	"github.com/reclamation-admin/agentic-browser-go/pkg/sitemap"
)

// NormalizeURL standardizes URLs to ensure node merging in the graph.
func NormalizeURL(rawURL string) string {
	u := strings.TrimSpace(rawURL)
	u = strings.ToLower(u)
	u = strings.TrimSuffix(u, "/")
	u = strings.TrimPrefix(u, "https://")
	u = strings.TrimPrefix(u, "http://")
	u = strings.TrimPrefix(u, "www.")
	return u
}

type Driver struct {
	sm *sitemap.SiteMap
}

// NewDriver initializes a connection to the SiteMap workspace (or defaults to local folder).
func NewDriver(uri, username, password string) (*Driver, error) {
	basePath := "sitemap_db"
	if !strings.HasPrefix(uri, "bolt:") && !strings.HasPrefix(uri, "neo4j:") && uri != "" {
		basePath = uri
	}
	sm, err := sitemap.Open(basePath)
	if err != nil {
		return nil, err
	}
	return &Driver{sm: sm}, nil
}

// UpsertPage creates or updates a Page node and its infrastructure triples in the Merkle SiteMap.
func (d *Driver) UpsertPage(ctx context.Context, url, title, summary, artifactPath string, links, scripts, cookies []string) error {
	urlHash := d.sm.RegisterString(url)
	titleHash := d.sm.RegisterString(title)
	summaryHash := d.sm.RegisterString(summary)
	artHash := d.sm.RegisterString(artifactPath)

	// Save Page Attributes
	d.saveTriple(urlHash, sitemap.PredicateURL, urlHash)
	d.saveTriple(urlHash, sitemap.PredicateTitle, titleHash)
	d.saveTriple(urlHash, sitemap.PredicateSummary, summaryHash)
	d.saveTriple(urlHash, sitemap.PredicateArtifactPath, artHash)

	// Outbound Links
	for _, link := range links {
		normTo := NormalizeURL(link)
		if normTo == "" {
			continue
		}
		linkHash := d.sm.RegisterString(link)
		d.saveTriple(urlHash, sitemap.PredicateLinksTo, linkHash)
	}

	// Hosted scripts
	for _, scriptURL := range scripts {
		scriptHash := d.sm.RegisterString(scriptURL)
		d.saveTriple(urlHash, sitemap.PredicateUsesScript, scriptHash)
	}

	// Cookies
	for _, cookie := range cookies {
		cookieHash := d.sm.RegisterString(cookie)
		d.saveTriple(urlHash, sitemap.PredicateUsesCookie, cookieHash)
	}

	return nil
}

func (d *Driver) saveTriple(sub uint64, pred uint16, obj uint64) {
	node := &sitemap.TripleNode{
		SubjectHash: sub,
		PredicateID: pred,
		ObjectHash:  obj,
	}
	_, _ = d.sm.SaveNode(node)
}

// EnsureFullTextIndex is a no-op fallback.
func (d *Driver) EnsureFullTextIndex(ctx context.Context) error {
	return nil
}

// Close is a no-op fallback.
func (d *Driver) Close() error {
	return nil
}
