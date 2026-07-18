package main

import (
	"context"
	"log"
	"os"
	"time"

	"github.com/chromedp/cdproto/page"
	"github.com/chromedp/chromedp"
)

func main() {
	log.Println("[Direct] Starting Direct CDP Real-World Test v24 (G2.com)...")

	userDataDir := "/app/ghost_chrome_profile_v18"
	extPath := "/app/extension/src"
	ua := "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
	
	opts := []chromedp.ExecAllocatorOption{
		chromedp.NoFirstRun,
		chromedp.NoDefaultBrowserCheck,
		chromedp.Flag("headless", "new"), 
		chromedp.Flag("test-type", true),
		chromedp.UserAgent(ua),
		chromedp.UserDataDir(userDataDir),
		chromedp.WindowSize(1920, 1080),
		chromedp.Flag("disable-session-crashed-bubble", true),
		chromedp.Flag("no-first-run", true),
		chromedp.Flag("no-default-browser-check", true),
		chromedp.Flag("disable-infobars", true),
		chromedp.Flag("load-extension", extPath),
		chromedp.Flag("disable-extensions-except", extPath),
		chromedp.Flag("disable-blink-features", "AutomationControlled"),
		chromedp.Flag("lang", "en-US"),
		chromedp.Flag("no-sandbox", true),
		chromedp.Flag("disable-setuid-sandbox", true),
		chromedp.Flag("disable-gpu-sandbox", true),
		chromedp.Flag("password-store", "basic"),
		chromedp.Flag("use-gl", "egl"),
		chromedp.Flag("enable-webgl", true),
		chromedp.Flag("ignore-gpu-blocklist", true),
		chromedp.Flag("disable-dev-shm-usage", true),
		chromedp.Flag("remote-debugging-port", "9222"),
		chromedp.Flag("force-renderer-accessibility", true),
	}

	allocCtx, allocCancel := chromedp.NewExecAllocator(context.Background(), opts...)
	defer allocCancel()

	ctx, cancel := chromedp.NewContext(allocCtx)
	defer cancel()

	log.Println("[Direct] Injecting Perfect Prototype WebGL Mock...")
	var buf []byte
	
	err := chromedp.Run(ctx,
		chromedp.ActionFunc(func(ctx context.Context) error {
			_, err := page.AddScriptToEvaluateOnNewDocument(`
				(function() {
					try {
						const originalGetContext = HTMLCanvasElement.prototype.getContext;
						HTMLCanvasElement.prototype.getContext = function(type, attributes) {
							const realCtx = originalGetContext.apply(this, arguments);
							if (realCtx) {
								if (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl') {
									const origGetParam = realCtx.getParameter;
									realCtx.getParameter = function(parameter) {
										if (parameter === 37445) return 'Google Inc. (NVIDIA Corporation)';
										if (parameter === 37446) return 'ANGLE (NVIDIA Corporation, NVIDIA GeForce RTX 3080/PCIe/SSE2, OpenGL 4.5.0)';
										return origGetParam.apply(this, arguments);
									};
								}
								return realCtx;
							}
							
							// If null, we create a perfect mock
							if (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl') {
								console.log("[Stealth] Providing MOCKED WebGL context for " + type);
								const mockContext = {
									getParameter: function(parameter) {
										if (parameter === 37445) return 'Google Inc. (NVIDIA Corporation)';
										if (parameter === 37446) return 'ANGLE (NVIDIA Corporation, NVIDIA GeForce RTX 3080/PCIe/SSE2, OpenGL 4.5.0)';
										if (parameter === 7936) return 'WebGL 1.0 (OpenGL ES 2.0 Chromium)';
										if (parameter === 7937) return 'WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)';
										if (parameter === 35661) return 16;
										if (parameter === 34930) return 16;
										return null;
									},
									getExtension: function(name) {
										if (name === 'WEBGL_debug_renderer_info') {
											return {
												UNMASKED_VENDOR_WEBGL: 37445,
												UNMASKED_RENDERER_WEBGL: 37446
											};
										}
										return null;
									},
									getSupportedExtensions: function() { return ['WEBGL_debug_renderer_info']; },
									clearColor: function() {},
									clear: function() {},
									createBuffer: function() { return {}; },
									bindBuffer: function() {},
									bufferData: function() {},
									enableVertexAttribArray: function() {},
									vertexAttribPointer: function() {},
									useProgram: function() {},
									drawArrays: function() {},
									canvas: this,
									getShaderPrecisionFormat: function() { return { rangeMin: 127, rangeMax: 127, precision: 23 }; }
								};
								
								// CRITICAL: Make it pass instanceof checks
								if (typeof WebGLRenderingContext !== 'undefined') {
									Object.setPrototypeOf(mockContext, WebGLRenderingContext.prototype);
								}
								
								return mockContext;
							}
							return realCtx;
						};
					} catch (e) {
						console.error("[Stealth] Error injecting WebGL mock:", e);
					}
				})();
			`).Do(ctx)
			return err
		}),
		chromedp.Navigate("https://www.ticketmaster.com/"),
		chromedp.Sleep(25*time.Second),
		chromedp.ActionFunc(func(ctx context.Context) error {
			log.Println("[Direct] Capturing Container Ticketmaster Result...")
			return nil
		}),
		chromedp.CaptureScreenshot(&buf),
	)

	if err != nil {
		log.Fatalf("[Direct] Audit failed: %v", err)
	}

	// 3. Save results
	screenshotPath := "/app/audit_result_v30.png"
	if err := os.WriteFile(screenshotPath, buf, 0644); err != nil {
		log.Printf("[Direct] Failed to save screenshot: %v", err)
	}

	log.Println("[Direct] Audit complete! Screenshot saved to " + screenshotPath)
}
