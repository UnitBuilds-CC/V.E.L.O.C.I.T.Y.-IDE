package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"time"

	"github.com/chromedp/cdproto/page"
	"github.com/chromedp/chromedp"
)

func main() {
	log.Println("[FullSweep] Starting Ultimate Anti-Bot Validation (Container Edition)...")

	// 1. Prepare Docker-specific flags
	opts := append(chromedp.DefaultExecAllocatorOptions[:],
		chromedp.NoFirstRun,
		chromedp.NoDefaultBrowserCheck,
		chromedp.Flag("headless", "new"),
		chromedp.NoSandbox,
		chromedp.DisableGPU, // Uses software rendering (llvmpipe)
	)

	// Add proxy if provided via env
	if proxy := os.Getenv("PROXY_SERVER"); proxy != "" {
		log.Printf("[FullSweep] Routing via Proxy: %s", proxy)
		opts = append(opts, chromedp.ProxyServer(proxy))
	}

	allocCtx, cancel := chromedp.NewExecAllocator(context.Background(), opts...)
	defer cancel()

	ctx, cancel := chromedp.NewContext(allocCtx)
	defer cancel()

	// Ensure timeout for the whole sweep
	ctx, cancel = context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()

	targets := []struct {
		Name string
		URL  string
	}{
		{"Kasada_Nike", "https://www.nike.com/"},
		{"DataDome_G2", "https://www.g2.com/products/squarespace/reviews"},
	}

	// Injected Stealth Script (The "Perfect Mock")
	stealthJS := `
		(function() {
			const mask = (obj, prop, value) => {
				try {
					Object.defineProperty(obj, prop, { get: () => value, configurable: true, enumerable: true });
				} catch (e) {
					try {
						delete obj[prop];
						Object.defineProperty(obj, prop, { get: () => value, configurable: true, enumerable: true });
					} catch (e2) {}
				}
			};

			const originalGetDescriptor = Object.getOwnPropertyDescriptor;
			Object.getOwnPropertyDescriptor = function(obj, prop) {
				if (obj === Navigator.prototype && prop === 'webdriver') {
					return { get: () => false, set: undefined, enumerable: true, configurable: true };
				}
				return originalGetDescriptor.apply(this, arguments);
			};

			mask(navigator, 'hardwareConcurrency', 12);
			mask(navigator, 'deviceMemory', 8);
			mask(navigator, 'platform', 'Linux x86_64');
			mask(navigator, 'languages', ['en-US', 'en']);
			mask(navigator, 'vendor', 'Google Inc.');
			
			mask(window.screen, 'width', 1536);
			mask(window.screen, 'height', 864);
			mask(window.screen, 'availWidth', 1536);
			mask(window.screen, 'availHeight', 816);
			mask(window.screen, 'colorDepth', 24);
			mask(window.screen, 'pixelDepth', 24);
			mask(window, 'innerWidth', 1536);
			mask(window, 'innerHeight', 816);
			mask(window, 'outerWidth', 1536);
			mask(window, 'outerHeight', 864);

			window.chrome = { runtime: {}, loadTimes: () => {}, csi: () => {}, app: {} };
			mask(window, 'chrome', window.chrome);

			if (typeof Intl !== 'undefined') {
				const originalResolvedOptions = Intl.DateTimeFormat.prototype.resolvedOptions;
				Intl.DateTimeFormat.prototype.resolvedOptions = function() {
					const options = originalResolvedOptions.apply(this, arguments);
					options.timeZone = 'America/Chicago';
					return options;
				};
			}

			if (navigator.permissions) {
				const originalQuery = navigator.permissions.query;
				navigator.permissions.query = function(parameters) {
					return parameters.name === 'notifications' 
						? Promise.resolve({ state: 'prompt', onchange: null })
						: originalQuery.apply(this, arguments);
				};
			}

			const fakePlugins = [
				{ name: 'Chrome PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
				{ name: 'Chromium PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
				{ name: 'Microsoft Edge PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
				{ name: 'PDF Viewer', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
				{ name: 'WebKit built-in PDF', filename: 'internal-pdf-viewer', description: 'Portable Document Format' }
			];
			mask(navigator, 'plugins', fakePlugins);
			mask(navigator, 'mimeTypes', [{ type: 'application/pdf', suffixes: 'pdf', description: 'Portable Document Format', enabledPlugin: fakePlugins[0] }]);

			const originalGetContext = HTMLCanvasElement.prototype.getContext;
			HTMLCanvasElement.prototype.getContext = function(type, attributes) {
				const realCtx = originalGetContext.apply(this, arguments);
				if (realCtx && (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl')) {
					const origGetParam = realCtx.getParameter;
					realCtx.getParameter = function(parameter) {
						if (parameter === 37445) return 'Google Inc. (NVIDIA Corporation)';
						if (parameter === 37446) return 'ANGLE (NVIDIA Corporation, NVIDIA GeForce RTX 3080/PCIe/SSE2, OpenGL 4.5.0)';
						return origGetParam.apply(this, arguments);
					};
				}
				return realCtx;
			};

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

			const originalToDataURL = HTMLCanvasElement.prototype.toDataURL;
			HTMLCanvasElement.prototype.toDataURL = function() {
				const context = this.getContext('2d');
				if (context) {
					const data = context.getImageData(0, 0, 1, 1);
					data.data[0] = (data.data[0] + 1) % 255;
					context.putImageData(data, 0, 0);
				}
				return originalToDataURL.apply(this, arguments);
			};
			
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

	// Register stealth script to run on every new document (frames, popups, navigations)
	chromedp.Run(ctx, chromedp.ActionFunc(func(ctx context.Context) error {
		_, err := page.AddScriptToEvaluateOnNewDocument(stealthJS).Do(ctx)
		return err
	}))

	for i, t := range targets {
		log.Printf("[FullSweep] [%d/%d] Testing %s: %s", i+1, len(targets), t.Name, t.URL)
		
		var buf []byte
		err := chromedp.Run(ctx,
			chromedp.Navigate(t.URL),
			chromedp.Sleep(15*time.Second), // Allow for JS challenges to settle
			chromedp.CaptureScreenshot(&buf),
		)

		if err != nil {
			log.Printf("[FullSweep] Error on %s: %v", t.Name, err)
			continue
		}

		filename := fmt.Sprintf("/app/sweep_%s.png", t.Name)
		if err := os.WriteFile(filename, buf, 0644); err != nil {
			log.Printf("[FullSweep] Failed to save %s: %v", filename, err)
		} else {
			log.Printf("[FullSweep] Success! Captured %s", filename)
		}
	}

	log.Println("[FullSweep] Ultimate Sweep Complete.")
}
