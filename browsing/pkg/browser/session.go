package browser

import (
	"context"
	"fmt"
	"os"
	goRuntime "runtime"
	"strings"
	"time"

	"github.com/chromedp/cdproto/accessibility"
	"github.com/chromedp/cdproto/cdp"
	"github.com/chromedp/cdproto/domsnapshot"
	"github.com/chromedp/cdproto/network"
	"github.com/chromedp/cdproto/page"
	"github.com/chromedp/cdproto/runtime"
	cdpTarget "github.com/chromedp/cdproto/target"
	"github.com/chromedp/chromedp"
	"github.com/reclamation-admin/agentic-browser-go/pkg/db"
)

// Session manages the CDP connection to a browser instance.
type Session struct {
	Ctx         context.Context
	Cancel      context.CancelFunc
	AllocCancel context.CancelFunc
	Port        int
	LastAom     []*PrunedNode

	MainCtx        context.Context
	MainCancel     context.CancelFunc
	MainTargetID   cdpTarget.ID
	ActiveTargetID cdpTarget.ID
	DbClient       *db.Client
}

// NewManagedSession starts a new Chrome instance and returns a session.
func NewManagedSession() (*Session, error) {
	userDataDir := "c:\\Users\\visse\\OneDrive\\Documentos\\Kimi Code\\velocity-workspace\\browsing\\chrome_profile"
	if goRuntime.GOOS == "linux" {
		userDataDir = "/app/ghost_chrome_profile"
	}
	os.MkdirAll(userDataDir, 0755)
	fmt.Fprintf(os.Stderr, "      [Browser] Starting Ghost Engine Hardened v2.1 (Headful)...\n")

	// extPath := "C:\\go-engine\\extension\\src"
	ua := "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36"
	if goRuntime.GOOS == "linux" {
		// extPath = "/app/extension/src"
		ua = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36"
	}

	opts := []chromedp.ExecAllocatorOption{
		chromedp.NoFirstRun,
		chromedp.NoDefaultBrowserCheck,
		chromedp.UserAgent(ua),
		chromedp.UserDataDir(userDataDir),
		chromedp.WindowSize(1920, 1080),
		chromedp.Flag("disable-gpu", true),
		chromedp.Flag("no-sandbox", true),
	}

	if goRuntime.GOOS == "linux" {
		opts = append(opts, chromedp.ExecPath("/usr/bin/chromium-browser"))
		opts = append(opts, chromedp.Flag("remote-debugging-address", "0.0.0.0"))
	} else if goRuntime.GOOS == "windows" {
		chromePath := "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
		if _, err := os.Stat(chromePath); err == nil {
			opts = append(opts, chromedp.ExecPath(chromePath))
		}
	}

	allocCtx, allocCancel := chromedp.NewExecAllocator(context.Background(), opts...)

	ctx, cancel := chromedp.NewContext(allocCtx)

	// Add console log listener
	chromedp.ListenTarget(ctx, func(ev interface{}) {
		if ev, ok := ev.(*runtime.EventConsoleAPICalled); ok {
			for _, arg := range ev.Args {
				fmt.Fprintf(os.Stderr, "      [Browser Console]: %s\n", arg.Value)
			}
		}
	})

	fmt.Fprintf(os.Stderr, "      [Browser] Initializing session...\n")
	var targetID cdpTarget.ID
	if err := chromedp.Run(ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		targetID = chromedp.FromContext(ctx).Target.TargetID
		// Enable auto-attach to discover iframes as targets
		return cdpTarget.SetAutoAttach(true, false).WithFlatten(true).Do(ctx)
	})); err != nil {
		fmt.Fprintf(os.Stderr, "      [Browser] FATAL ERROR during Run: %v\n", err)
		cancel()
		allocCancel()
		return nil, fmt.Errorf("failed to start browser: %w", err)
	}

	if err := chromedp.Run(ctx,
		network.Enable(),
		runtime.AddBinding("client_data"),
		chromedp.ActionFunc(func(ctx context.Context) error {
			maskScript := `
				(function() {
					const maskedFunctions = new Map();
					const originalToString = Function.prototype.toString;
					Function.prototype.toString = function() {
						if (maskedFunctions.has(this)) {
							return maskedFunctions.get(this);
						}
						return originalToString.call(this);
					};
					maskedFunctions.set(Function.prototype.toString, "function toString() { [native code] }");

					function mask(obj, prop, value) {
						try {
							Object.defineProperty(obj, prop, {
								get: () => value,
								configurable: true,
								enumerable: true
							});
						} catch (e) {
							try {
								delete obj[prop];
								Object.defineProperty(obj, prop, {
									get: () => value,
									configurable: true,
									enumerable: true
								});
							} catch (e2) {}
						}
					}

					// 1. Ghost Descriptor Bypass
					const originalGetDescriptor = Object.getOwnPropertyDescriptor;
					Object.getOwnPropertyDescriptor = function(obj, prop) {
						if ((obj === Navigator.prototype || obj === navigator) && prop === 'webdriver') {
							return {
								get: () => false,
								set: undefined,
								enumerable: true,
								configurable: true
							};
						}
						return originalGetDescriptor.apply(this, arguments);
					};
					maskedFunctions.set(Object.getOwnPropertyDescriptor, "function getOwnPropertyDescriptor() { [native code] }");

					// 2. Apply Masks
					// Use a Proxy on navigator to intercept 'webdriver'
					const navigatorProxy = new Proxy(navigator, {
						get: (target, prop) => {
							if (prop === 'webdriver') return false;
							const val = target[prop];
							return typeof val === 'function' ? val.bind(target) : val;
						}
					});
					mask(window, 'navigator', navigatorProxy);

					mask(Navigator.prototype, 'webdriver', false);
					mask(navigator, 'hardwareConcurrency', 8);
					mask(navigator, 'deviceMemory', 8);
					
					// 2.5 window.chrome Mock
					window.chrome = {
						runtime: {},
						loadTimes: function() {},
						csi: function() {},
						app: {}
					};
					mask(window, 'chrome', window.chrome);

					// 3. UserAgentData Mock (Critical for DataDome)
					if (navigator.userAgentData) {
						mask(navigator.userAgentData, 'platform', 'Linux');
						mask(navigator.userAgentData, 'mobile', false);
						mask(navigator.userAgentData, 'brands', [
							{ brand: 'Google Chrome', version: '131' },
							{ brand: 'Chromium', version: '131' },
							{ brand: 'Not_A Brand', version: '24' }
						]);
						mask(navigator.userAgentData, 'fullVersionList', [
							{ brand: 'Google Chrome', version: '131.0.6778.85' },
							{ brand: 'Chromium', version: '131.0.6778.85' },
							{ brand: 'Not_A Brand', version: '24.0.0.0' }
						]);
					}

					mask(navigator, 'platform', 'Linux x86_64');
					mask(navigator, 'languages', ['en-US', 'en']);
					
					// 4. Plugins Mock (Real browsers have these)
					const fakePlugins = Object.create(PluginArray.prototype);
					Object.defineProperty(fakePlugins, 'length', { get: () => 5 });
					const pluginsList = [
						{ name: 'Chrome PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
						{ name: 'Chromium PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
						{ name: 'Microsoft Edge PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
						{ name: 'PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
						{ name: 'WebKit built-in PDF', filename: 'internal-pdf-viewer', description: 'Portable Document Format' }
					];
					pluginsList.forEach((p, i) => {
						const plugin = Object.create(Plugin.prototype);
						mask(plugin, 'name', p.name);
						mask(plugin, 'filename', p.filename);
						mask(plugin, 'description', p.description);
						fakePlugins[i] = plugin;
						fakePlugins[p.name] = plugin;
					});
					mask(navigator, 'plugins', fakePlugins);
					mask(navigator, 'mimeTypes', [{ type: 'application/pdf', suffixes: 'pdf', description: 'Portable Document Format', enabledPlugin: fakePlugins[0] }]);
					const ensureCanvasMetrics = (canvas) => {
						if (!canvas) return null;
						if (!Object.prototype.hasOwnProperty.call(canvas, '__velocityCanvasMetrics') || !canvas.__velocityCanvasMetrics) {
							Object.defineProperty(canvas, '__velocityCanvasMetrics', {
								value: {
									contexts: [],
									textOpCount: 0,
									imageOpCount: 0,
									webglDrawCount: 0,
									readbackCount: 0,
									likelyAnimated: false,
									textOps: []
								},
								configurable: true,
								enumerable: false,
								writable: true
							});
						}
						return canvas.__velocityCanvasMetrics;
					};
					const rememberCanvasContext = (metrics, type) => {
						if (!metrics) return;
						const kind = String(type || '').trim().toLowerCase();
						if (!kind) return;
						if (!Array.isArray(metrics.contexts)) metrics.contexts = [];
						if (!metrics.contexts.includes(kind)) metrics.contexts.push(kind);
					};
					const wrapMethod = (obj, key, wrapperFactory) => {
						if (!obj || typeof obj[key] !== 'function') return;
						const original = obj[key];
						if (original.__velocityWrapped) return;
						const wrapped = wrapperFactory(original);
						wrapped.__velocityWrapped = true;
						obj[key] = wrapped;
					};
					const wrapCanvasContext = (canvas, type, realCtx) => {
						const metrics = ensureCanvasMetrics(canvas);
						rememberCanvasContext(metrics, type);
						if (!realCtx || !metrics) return realCtx;
						const normalizedType = String(type || '').trim().toLowerCase();
						if (realCtx.__velocityCanvasWrapped) return realCtx;
						Object.defineProperty(realCtx, '__velocityCanvasWrapped', { value: true, configurable: true });
						if (normalizedType === '2d') {
							wrapMethod(realCtx, 'fillText', (original) => function() {
								metrics.textOpCount = Number(metrics.textOpCount || 0) + 1;
								if (metrics.textOps.length < 8) metrics.textOps.push({ text: String(arguments[0] || '').slice(0, 80) });
								return original.apply(this, arguments);
							});
							wrapMethod(realCtx, 'strokeText', (original) => function() {
								metrics.textOpCount = Number(metrics.textOpCount || 0) + 1;
								if (metrics.textOps.length < 8) metrics.textOps.push({ text: String(arguments[0] || '').slice(0, 80) });
								return original.apply(this, arguments);
							});
							wrapMethod(realCtx, 'drawImage', (original) => function() {
								metrics.imageOpCount = Number(metrics.imageOpCount || 0) + 1;
								return original.apply(this, arguments);
							});
							wrapMethod(realCtx, 'getImageData', (original) => function() {
								metrics.readbackCount = Number(metrics.readbackCount || 0) + 1;
								return original.apply(this, arguments);
							});
							wrapMethod(realCtx, 'putImageData', (original) => function() {
								metrics.imageOpCount = Number(metrics.imageOpCount || 0) + 1;
								return original.apply(this, arguments);
							});
						} else if (normalizedType === 'webgl' || normalizedType === 'webgl2' || normalizedType === 'experimental-webgl') {
							wrapMethod(realCtx, 'drawArrays', (original) => function() {
								metrics.webglDrawCount = Number(metrics.webglDrawCount || 0) + 1;
								return original.apply(this, arguments);
							});
							wrapMethod(realCtx, 'drawElements', (original) => function() {
								metrics.webglDrawCount = Number(metrics.webglDrawCount || 0) + 1;
								return original.apply(this, arguments);
							});
							wrapMethod(realCtx, 'readPixels', (original) => function() {
								metrics.readbackCount = Number(metrics.readbackCount || 0) + 1;
								return original.apply(this, arguments);
							});
							const origGetParam = realCtx.getParameter;
							realCtx.getParameter = function(parameter) {
								if (parameter === 37445) return 'Google Inc. (NVIDIA Corporation)';
								if (parameter === 37446) return 'ANGLE (NVIDIA Corporation, NVIDIA GeForce RTX 3080/PCIe/SSE2, OpenGL 4.5.0)';
								return origGetParam.apply(this, arguments);
							};
						}
						return realCtx;
					};
					const originalGetContext = HTMLCanvasElement.prototype.getContext;
					HTMLCanvasElement.prototype.getContext = function(type, attributes) {
						const metrics = ensureCanvasMetrics(this);
						rememberCanvasContext(metrics, type);
						const realCtx = originalGetContext.apply(this, arguments);
						return wrapCanvasContext(this, type, realCtx);
					};
					const originalRequestAnimationFrame = window.requestAnimationFrame;
					window.requestAnimationFrame = function(callback) {
						return originalRequestAnimationFrame.call(this, function(timestamp) {
							try {
								for (const canvas of Array.from(document.querySelectorAll('canvas'))) {
									const metrics = ensureCanvasMetrics(canvas);
									if (metrics && (metrics.webglDrawCount > 0 || metrics.imageOpCount > 0 || metrics.textOpCount > 0)) {
										metrics.likelyAnimated = true;
									}
								}
							} catch (e) {}
							return callback(timestamp);
						});
					};

					// 5. Canvas Readout Noise (Final DataDome Kill-Switch)
					const originalToDataURL = HTMLCanvasElement.prototype.toDataURL;
					HTMLCanvasElement.prototype.toDataURL = function() {
						const metrics = ensureCanvasMetrics(this);
						if (metrics) metrics.readbackCount = Number(metrics.readbackCount || 0) + 1;
						const context = this.getContext('2d');
						if (context) {
							const data = context.getImageData(0, 0, 1, 1);
							data.data[0] = (data.data[0] + 1) % 255;
							context.putImageData(data, 0, 0);
						}
						return originalToDataURL.apply(this, arguments);
					};
					maskedFunctions.set(HTMLCanvasElement.prototype.getContext, "function getContext() { [native code] }");
					
					const clean = () => {
						try {
							for (const p in window) {
								if (p.includes('cdc_') || p.includes('__node_type')) {
									delete window[p];
								}
							}
						} catch (e) {}
					};
					clean();
					setTimeout(clean, 500);
					setTimeout(clean, 2000);
				})();
			`
			platform := "Windows"
			platformFull := "Win32"
			if goRuntime.GOOS == "linux" {
				platform = "Linux"
				platformFull = "Linux x86_64"
			}
			maskScript = strings.ReplaceAll(maskScript, "Linux x86_64", platformFull)
			maskScript = strings.ReplaceAll(maskScript, "Linux", platform)
			maskScript = strings.ReplaceAll(maskScript, "131", "147")
			maskScript = strings.ReplaceAll(maskScript, "131.0.6778.85", "147.0.7727.138")

			_, err := page.AddScriptToEvaluateOnNewDocument(maskScript).Do(ctx)
			return err
		}),
	); err != nil {
		fmt.Fprintf(os.Stderr, "      [Warning] Failed to initialize stealth: %v\n", err)
	}

	dbClient, err := db.NewClient()
	if err != nil {
		fmt.Fprintf(os.Stderr, "      [Warning] Failed to initialize Neo4J client: %v\n", err)
	}

	return &Session{
		Ctx:            ctx,
		Cancel:         cancel,
		AllocCancel:    allocCancel,
		MainCtx:        ctx,
		MainCancel:     cancel,
		MainTargetID:   targetID,
		ActiveTargetID: targetID,
		DbClient:       dbClient,
	}, nil
}

// NewSession connects to an existing Chrome instance on the specified port.
func NewSession(port int) (*Session, error) {
	devtoolsURL := fmt.Sprintf("http://127.0.0.1:%d", port)

	// Create an allocator context that points to the remote browser
	allocCtx, _ := chromedp.NewRemoteAllocator(context.Background(), devtoolsURL)

	// Create a context from the allocator
	ctx, cancel := chromedp.NewContext(allocCtx)

	// Add console log listener
	chromedp.ListenTarget(ctx, func(ev interface{}) {
		if ev, ok := ev.(*runtime.EventConsoleAPICalled); ok {
			for _, arg := range ev.Args {
				fmt.Fprintf(os.Stderr, "      [Browser Console]: %s\n", arg.Value)
			}
		}
	})

	var targetID cdpTarget.ID
	if err := chromedp.Run(ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		targetID = chromedp.FromContext(ctx).Target.TargetID
		return nil
	})); err != nil {
		cancel()
		return nil, err
	}

	if err := chromedp.Run(ctx,
		network.Enable(),
		// [STEALTH] Overwrite navigator.webdriver to further hide automation
		runtime.AddBinding("antigravity"),
		chromedp.ActionFunc(func(ctx context.Context) error {
			_, err := page.AddScriptToEvaluateOnNewDocument(`
				Object.defineProperty(navigator, 'webdriver', {
					get: () => undefined
				});
				window.chrome = {
					runtime: {}
				};
			`).Do(ctx)
			return err
		}),
	); err != nil {
		fmt.Fprintf(os.Stderr, "      [Warning] Failed to set ad-blocking: %v\n", err)
	}

	dbClient, err := db.NewClient()
	if err != nil {
		fmt.Fprintf(os.Stderr, "      [Warning] Failed to initialize Neo4J client: %v\n", err)
	}

	return &Session{
		Ctx:            ctx,
		Cancel:         cancel,
		Port:           port,
		MainTargetID:   targetID,
		ActiveTargetID: targetID,
		DbClient:       dbClient,
	}, nil
}

// Navigate tells the browser to go to a specific URL.
func (s *Session) Navigate(url string) error {
	return chromedp.Run(s.Ctx, chromedp.Navigate(url))
}

// GetAom retrieves and prunes the accessibility tree with optimized batch perception.
func (s *Session) GetAom(cfg AomConfig) (string, error) {
	start := time.Now()
	var nodes []*accessibility.Node
	var snapshots []*domsnapshot.DocumentSnapshot
	var stringsTable []string
	var url string
	var viewportHeight int64

	// Add 5s timeout for the entire AOM fetch to avoid hanging the tool
	ctx, cancel := context.WithTimeout(s.Ctx, 5*time.Second)
	defer cancel()

	err := chromedp.Run(ctx,
		chromedp.Evaluate("window.innerHeight", &viewportHeight),
		chromedp.ActionFunc(func(ctx context.Context) error {
			var err error
			// 1. Get Main AXTree
			nodes, err = accessibility.GetFullAXTree().Do(ctx)
			if err != nil {
				return err
			}

			// 2. Get DOM Snapshots for metadata
			var innerErr error
			snapshots, stringsTable, innerErr = domsnapshot.CaptureSnapshot([]string{
				"color", "background-color", "font-size", "opacity", "display", "cursor",
			}).WithIncludeDOMRects(true).Do(ctx)
			if innerErr != nil {
				return innerErr
			}
			return nil
		}),
		chromedp.Location(&url),
	)
	if err != nil {
		return "", err
	}

	processingStart := time.Now()
	// Build DOM lookup map from snapshot
	type nodeMeta struct {
		Attrs      map[string]string
		X, Y, W, H int64
		Style      string
	}
	domMap := make(map[cdp.BackendNodeID]nodeMeta)

	if len(snapshots) > 0 {
		doc := snapshots[0]
		if doc.Nodes != nil && doc.Layout != nil {
			dNodes := doc.Nodes
			layout := doc.Layout

			// Map layout to nodes
			nodeToLayout := make(map[int]int)
			if layout.NodeIndex != nil {
				for i, nodeIdx := range layout.NodeIndex {
					nodeToLayout[int(nodeIdx)] = i
				}
			}

			// Whitelist style names for faster lookup
			styleNames := []string{"color", "background-color", "font-size", "opacity", "display", "cursor"}

			if dNodes.BackendNodeID != nil {
				for i, bid := range dNodes.BackendNodeID {
					meta := nodeMeta{Attrs: make(map[string]string)}

					// Attributes
					if dNodes.Attributes != nil && i < len(dNodes.Attributes) && dNodes.Attributes[i] != nil {
						for j := 0; j+1 < len(dNodes.Attributes[i]); j += 2 {
							attrNameIdx := dNodes.Attributes[i][j]
							attrValIdx := dNodes.Attributes[i][j+1]
							if attrNameIdx >= 0 && int(attrNameIdx) < len(stringsTable) &&
								attrValIdx >= 0 && int(attrValIdx) < len(stringsTable) {
								meta.Attrs[stringsTable[attrNameIdx]] = stringsTable[attrValIdx]
							}
						}
					}

					// Layout & Styles
					if lIdx, found := nodeToLayout[i]; found {
						if layout.Bounds != nil && lIdx < len(layout.Bounds) {
							bounds := layout.Bounds[lIdx]
							if len(bounds) >= 4 {
								meta.X = int64(bounds[0])
								meta.Y = int64(bounds[1])
								meta.W = int64(bounds[2])
								meta.H = int64(bounds[3])
							}
						}

						if layout.Styles != nil && lIdx < len(layout.Styles) {
							var res []string
							for j, valIdx := range layout.Styles[lIdx] {
								if valIdx >= 0 && int(valIdx) < len(stringsTable) && j < len(styleNames) {
									name := styleNames[j]
									val := stringsTable[valIdx]
									if name == "opacity" && val != "1" {
										res = append(res, "faded")
									}
									if name == "display" && val == "none" {
										res = append(res, "hidden")
									}
									if name == "cursor" && val == "pointer" {
										res = append(res, "clickable")
									}
								}
							}
							meta.Style = strings.Join(res, ",")
						}
					}

					domMap[bid] = meta
				}
			}
		}
	}

	if len(nodes) == 0 {
		return "", fmt.Errorf("no AOM nodes found")
	}
	axRoot := convertAxTree(nodes)
	s.LastAom = PruneAom([]*PrunedNode{axRoot})

	// Enrich AOM nodes
	var enrich func([]*PrunedNode)
	enrich = func(ns []*PrunedNode) {
		for _, n := range ns {
			if meta, ok := domMap[cdp.BackendNodeID(n.BackendID)]; ok {
				n.Attrs = meta.Attrs
				n.X = meta.X
				n.Y = meta.Y
				n.W = meta.W
				n.H = meta.H
				n.Style = meta.Style
				// Check visibility
				if n.Y > viewportHeight || (n.Y+n.H) < 0 {
					n.IsOffscreen = true
				}
			}
			enrich(n.Children)
		}
	}
	enrich(s.LastAom)
	fmt.Fprintf(os.Stderr, "      [Telemetry] Go Processing: %v\n", time.Since(processingStart))

	// Save to Neo4J if client is available
	if s.DbClient != nil && len(s.LastAom) > 0 {
		dbNode := convertToDbNode(s.LastAom[0])
		if err := s.DbClient.SaveAOM(s.Ctx, dbNode, url); err != nil {
			fmt.Fprintf(os.Stderr, "      [Warning] Failed to save AOM to Neo4J: %v\n", err)
		} else {
			fmt.Fprintf(os.Stderr, "      [Telemetry] AOM saved to Neo4J for URL: %s\n", url)
		}
	}

	currentLen := 0
	if cfg.MaxLength == 0 {
		cfg.MaxLength = 95000
	}

	// Always use summarized version for the model response
	cfg.Summarized = true

	res := SerializeAom(s.LastAom, 0, &currentLen, cfg)
	fmt.Fprintf(os.Stderr, "      [Telemetry] Total GetAom: %v\n", time.Since(start))
	return res, nil
}

// GetSummarizedAomFast uses JavaScript to quickly extract interactive elements.
func (s *Session) GetSummarizedAomFast() (string, error) {
	var result string
	script := `
		(function() {
			const elements = document.querySelectorAll('a, button, input, select, textarea, [role="button"]');
			let res = "";
			let count = 0;
			for (let i = 0; i < elements.length; i++) {
				if (count >= 200) break; // Limit to 200 visible elements
				const el = elements[i];
				const rect = el.getBoundingClientRect();
				if (rect.width === 0 || rect.height === 0) continue; // Skip invisible
				
				const role = el.tagName.toLowerCase();
				const name = el.innerText || el.placeholder || el.value || el.ariaLabel || "";
				const id = el.id || i;
				
				res += "[" + role + " " + id + "] " + JSON.stringify(name) + " @(" + Math.round(rect.x) + "," + Math.round(rect.y) + "," + Math.round(rect.width) + "," + Math.round(rect.height) + ")\n";
				count++;
			}
			return res;
		})()
	`
	err := chromedp.Run(s.Ctx, chromedp.Evaluate(script, &result))
	return result, err
}

func convertAxTree(rawNodes []*accessibility.Node) *PrunedNode {
	if len(rawNodes) == 0 {
		return nil
	}

	nodeMap := make(map[accessibility.NodeID]*PrunedNode)
	for _, raw := range rawNodes {
		node := &PrunedNode{
			NodeID:    fmt.Sprintf("%d", raw.BackendDOMNodeID),
			BackendID: int64(raw.BackendDOMNodeID),
			Children:  []*PrunedNode{},
		}

		if raw.Role != nil {
			node.Role = strings.Trim(fmt.Sprintf("%v", raw.Role.Value), "\"")
		}
		if raw.Name != nil {
			node.Name = strings.Trim(fmt.Sprintf("%v", raw.Name.Value), "\"")
		}
		if raw.Value != nil {
			node.Value = strings.Trim(fmt.Sprintf("%v", raw.Value.Value), "\"")
		}

		nodeMap[raw.NodeID] = node
	}

	var root *PrunedNode
	for _, raw := range rawNodes {
		node := nodeMap[raw.NodeID]
		if root == nil {
			root = node
		}
		for _, childID := range raw.ChildIDs {
			if child, ok := nodeMap[childID]; ok {
				node.Children = append(node.Children, child)
			}
		}
	}

	return root
}

func convertToDbNode(n *PrunedNode) *db.AOMNode {
	if n == nil {
		return nil
	}
	dbNode := &db.AOMNode{
		NodeID:      n.NodeID,
		BackendID:   n.BackendID,
		Role:        n.Role,
		Name:        n.Name,
		Value:       n.Value,
		IsOffscreen: n.IsOffscreen,
		X:           n.X,
		Y:           n.Y,
		W:           n.W,
		H:           n.H,
		Style:       n.Style,
		Attrs:       n.Attrs,
	}
	for _, child := range n.Children {
		dbNode.Children = append(dbNode.Children, convertToDbNode(child))
	}
	return dbNode
}

// WaitForStability waits for the DOM to stop mutating using an injected observer.
func (s *Session) WaitForStability(timeout time.Duration) error {
	return s.customWait(timeout, 500)
}

// QuickWait is a faster version of WaitForStability with a shorter debounce.
func (s *Session) QuickWait(timeout time.Duration) error {
	return s.customWait(timeout, 150)
}

func (s *Session) customWait(timeout time.Duration, debounceMs int) error {
	maxWaitMs := 2000 // Lower default to 2s for agentic responsiveness
	if timeout > 0 {
		maxWaitMs = int(timeout.Milliseconds())
	}

	script := fmt.Sprintf(`
		(function() {
			return new Promise((resolve) => {
				let maxWait = setTimeout(() => {
					resolve("timeout");
				}, %d);

				let observer = new MutationObserver(() => {
					clearTimeout(maxWait);
					maxWait = setTimeout(() => {
						observer.disconnect();
						resolve("stable");
					}, %d); 
				});
				observer.observe(document, { attributes: true, childList: true, subtree: true });
			});
		})()
	`, maxWaitMs, debounceMs)

	start := time.Now()
	err := chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		_, _, err := runtime.Evaluate(script).WithAwaitPromise(true).Do(ctx)
		return err
	}))
	fmt.Fprintf(os.Stderr, "      [Telemetry] Mutation Wait (%dms): %v\n", debounceMs, time.Since(start))
	return err
}

// WaitUntilElementExists waits until a node with the given name/role appears.
func (s *Session) WaitUntilElementExists(role string, namePart string, timeout time.Duration) error {
	ctx, cancel := context.WithTimeout(s.Ctx, timeout)
	defer cancel()

	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
			_, err := s.GetAom(AomConfig{})
			if err == nil {
				// Search in LastAom
				var find func([]*PrunedNode) bool
				find = func(ns []*PrunedNode) bool {
					for _, n := range ns {
						if (role == "" || strings.EqualFold(n.Role, role)) &&
							(namePart == "" || strings.Contains(strings.ToLower(n.Name), strings.ToLower(namePart))) {
							return true
						}
						if find(n.Children) {
							return true
						}
					}
					return false
				}
				if find(s.LastAom) {
					return nil
				}
			}
			time.Sleep(200 * time.Millisecond) // Poll every 200ms
		}
	}
}

// CaptureScreenshot captures a PNG of the current viewport.
func (s *Session) CaptureScreenshot() ([]byte, error) {
	var buf []byte
	ctx, cancel := context.WithTimeout(s.Ctx, 30*time.Second)
	defer cancel()

	if err := chromedp.Run(ctx, chromedp.CaptureScreenshot(&buf)); err != nil {
		return nil, err
	}
	return buf, nil
}

// CurrentURL returns the currently loaded page URL.
func (s *Session) CurrentURL() (string, error) {
	var currentURL string
	ctx, cancel := context.WithTimeout(s.Ctx, 10*time.Second)
	defer cancel()

	if err := chromedp.Run(ctx, chromedp.Location(&currentURL)); err != nil {
		return "", err
	}
	return currentURL, nil
}

// TakeScreenshot captures a PNG of the current viewport.
func (s *Session) TakeScreenshot(path string) error {
	buf, err := s.CaptureScreenshot()
	if err != nil {
		return err
	}
	return os.WriteFile(path, buf, 0644)
}

// GetPageText extracts all visible text from the current AOM tree.
func (s *Session) GetPageText() string {
	if len(s.LastAom) == 0 {
		return ""
	}
	var sb strings.Builder
	var traverse func([]*PrunedNode)
	traverse = func(ns []*PrunedNode) {
		for _, n := range ns {
			if n.Name != "" && n.Role != "link" && n.Role != "button" {
				sb.WriteString(n.Name)
				sb.WriteString("\n")
			}
			traverse(n.Children)
		}
	}
	traverse(s.LastAom)
	return sb.String()
}

// GetLinks extracts all unique URLs found in link roles within the AOM tree.
func (s *Session) GetLinks() []string {
	links := make(map[string]bool)
	var traverse func([]*PrunedNode)
	traverse = func(ns []*PrunedNode) {
		for _, n := range ns {
			if n.Role == "link" {
				if href, ok := n.Attrs["href"]; ok && href != "" {
					links[href] = true
				}
			}
			traverse(n.Children)
		}
	}
	traverse(s.LastAom)

	var res []string
	for l := range links {
		res = append(res, l)
	}
	return res
}

func (s *Session) Close() {
	if s.Cancel != nil {
		s.Cancel()
	}
	if s.AllocCancel != nil {
		s.AllocCancel()
	}
}

func (s *Session) IsAlive() bool {
	if s.Ctx == nil {
		return false
	}
	select {
	case <-s.Ctx.Done():
		return false
	default:
		return true
	}
}

// GetScripts extracts all external script URLs from the current page.
func (s *Session) GetScripts() []string {
	var scripts []string
	scriptQuery := `
		Array.from(document.querySelectorAll('script[src]'))
			.map(s => s.src)
			.filter(src => {
				try { return new URL(src).hostname !== window.location.hostname; }
				catch(e) { return false; }
			})
	`
	if err := chromedp.Run(s.Ctx, chromedp.Evaluate(scriptQuery, &scripts)); err != nil {
		fmt.Fprintf(os.Stderr, "[Warning] Failed to extract scripts: %v\n", err)
	}
	return scripts
}

// GetCookies extracts all cookie names and domains from the current session.
func (s *Session) GetCookies() []string {
	var cookies []*network.Cookie
	if err := chromedp.Run(s.Ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		var err error
		cookies, err = network.GetCookies().Do(ctx)
		return err
	})); err != nil {
		fmt.Fprintf(os.Stderr, "[Warning] Failed to extract cookies: %v\n", err)
		return nil
	}

	var cookieNames []string
	for _, c := range cookies {
		cookieNames = append(cookieNames, fmt.Sprintf("%s (%s)", c.Name, c.Domain))
	}
	return cookieNames
}

// ExtractFields finds all input, select, and textarea elements and returns a map of field names to CSS selectors.
func (s *Session) ExtractFields() (map[string]string, error) {
	var nodes []*cdp.Node
	// We use a timeout to avoid hanging if the page is weird
	ctx, cancel := context.WithTimeout(s.Ctx, 5*time.Second)
	defer cancel()

	err := chromedp.Run(ctx, chromedp.Nodes(`input, select, textarea`, &nodes, chromedp.ByQueryAll))
	if err != nil {
		return nil, err
	}

	fields := make(map[string]string)
	for _, n := range nodes {
		id := n.AttributeValue("id")
		name := n.AttributeValue("name")
		placeholder := n.AttributeValue("placeholder")

		// Determine a friendly name for the field
		key := name
		if key == "" {
			key = id
		}
		if key == "" {
			key = placeholder
		}
		if key == "" {
			key = n.LocalName
		}

		// Build a simple CSS selector
		selector := n.LocalName
		if id != "" {
			selector += "#" + id
		} else if name != "" {
			selector += "[name='" + name + "']"
		} else if placeholder != "" {
			selector += "[placeholder='" + placeholder + "']"
		}

		// If we still don't have a unique key, use the selector itself
		if key == n.LocalName {
			key = selector
		}

		fields[key] = selector
	}
	return fields, nil
}
