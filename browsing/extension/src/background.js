let nativePort = null;
let automationRoot = null;

function logRemote(msg) {
    fetch('http://localhost:8889/log', {
        method: 'POST',
        body: JSON.stringify(msg)
    }).catch(() => {});
}

console.log("[Ghost] Extension Loaded. ID:", chrome.runtime.id);

function sendToNative(msg) {
    if (nativePort) {
        nativePort.postMessage(msg);
    }
}

// 1. Traffic Monitoring Logic
chrome.webRequest.onBeforeSendHeaders.addListener(
    (details) => {
        if (details.url.includes("g2.com") || details.url.includes("httpbin.org") || details.url.includes("localhost:8888")) {
            sendToNative({
                type: "TRAFFIC_LOG",
                data: {
                    direction: "REQUEST",
                    url: details.url,
                    method: details.method,
                    headers: details.requestHeaders
                }
            });
        }
    },
    { urls: ["<all_urls>"] },
    ["requestHeaders"]
);

chrome.webRequest.onHeadersReceived.addListener(
    (details) => {
        if (details.url.includes("g2.com") || details.url.includes("httpbin.org") || details.url.includes("localhost:8888")) {
            sendToNative({
                type: "TRAFFIC_LOG",
                data: {
                    direction: "RESPONSE",
                    url: details.url,
                    status: details.statusLine,
                    headers: details.responseHeaders
                }
            });
        }
    },
    { urls: ["<all_urls>"] },
    ["responseHeaders"]
);

function connectNative() {
    try {
        console.log("[Ghost] Attempting to connect to Native Host...");
        nativePort = chrome.runtime.connectNative('com.ghost.bridge.relay');
        
        nativePort.onMessage.addListener((msg) => {
            handleMessage(msg);
        });

        nativePort.onDisconnect.addListener(() => {
            console.error("[Ghost] Native Host Disconnected:", chrome.runtime.lastError);
            nativePort = null;
            setTimeout(connectNative, 5000);
        });
    } catch (e) {
        console.error("[Ghost] Connection Exception:", e);
    }
}

async function handleMessage(msg) {
    if (msg.type === "NAVIGATE") {
        const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
        if (tab) {
            await chrome.tabs.update(tab.id, { url: msg.url });
        }
    } else if (msg.type === "GET_AOM") {
        // PIVOT: Using Native Automation API instead of Script Injection
        chrome.automation.getTree((root) => {
            automationRoot = root;
            const simplified = serializeNode(root);
            sendToNative({ type: "AOM_RESULT", data: JSON.stringify(simplified) });
        });
    } else if (msg.type === "NATIVE_ACTION") {
        // PIVOT: Performing native actions via Accessibility Layer
        if (!automationRoot) {
            sendToNative({ type: "ERROR", msg: "No AOM tree loaded. Call GET_AOM first." });
            return;
        }
        const target = findNodeByIdentifier(automationRoot, msg.identifier);
        if (target) {
            if (msg.action === "CLICK") {
                target.doDefault();
            } else if (msg.action === "FOCUS") {
                target.focus();
            } else if (msg.action === "SET_VALUE") {
                target.setValue(msg.value);
            }
            sendToNative({ type: "ACTION_SUCCESS" });
        } else {
            sendToNative({ type: "ERROR", msg: "Node not found: " + msg.identifier });
        }
    } else if (msg.type === "SCREENSHOT") {
        chrome.tabs.captureVisibleTab(null, { format: "png" }, (dataUrl) => {
            sendToNative({ type: "SCREENSHOT_RESULT", data: dataUrl });
        });
    }
}

// Recursively serialize the native Automation tree into a simple format for Go
function serializeNode(node) {
    if (!node) return null;
    const res = {
        role: node.role || "unknown",
        name: node.name || "",
        x: node.location ? Math.round(node.location.left) : 0,
        y: node.location ? Math.round(node.location.top) : 0,
        w: node.location ? Math.round(node.location.width) : 0,
        h: node.location ? Math.round(node.location.height) : 0,
        children: []
    };
    
    // Only walk visible nodes to keep the tree manageable
    const children = node.children || [];
    for (const child of children) {
        const childRes = serializeNode(child);
        if (childRes) res.children.push(childRes);
    }
    return res;
}

// Helper to find a node in the native tree by name or role
function findNodeByIdentifier(node, identifier) {
    if (node.name === identifier || node.role === identifier) return node;
    const children = node.children || [];
    for (const child of children) {
        const found = findNodeByIdentifier(child, identifier);
        if (found) return found;
    }
    return null;
}

connectNative();
