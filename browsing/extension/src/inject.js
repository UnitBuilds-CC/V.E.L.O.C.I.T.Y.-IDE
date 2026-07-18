(function() {
    console.log("GHOST BRIDGE INJECTED - HARDENING ACTIVE");
    // 1. Advanced Function.toString masking
    const originalToString = Function.prototype.toString;
    const maskedFunctions = new Map();

    Function.prototype.toString = function() {
        if (maskedFunctions.has(this)) return maskedFunctions.get(this);
        return originalToString.call(this);
    };
    maskedFunctions.set(Function.prototype.toString, "function toString() { [native code] }");

    function mask(obj, prop, value) {
        const proto = Object.getPrototypeOf(obj);
        const originalDescriptor = Object.getOwnPropertyDescriptor(proto || obj, prop);
        
        const getter = () => value;
        maskedFunctions.set(getter, `function get ${prop}() { [native code] }`);

        Object.defineProperty(obj, prop, {
            get: getter,
            configurable: true,
            enumerable: true
        });
    }

    // 2. Truthful Linux Identity (Align with container)
    // Ghost Descriptor Bypass
    const webdriverGetter = () => false;
    maskedFunctions.set(webdriverGetter, "function get webdriver() { [native code] }");

    const originalGetDescriptor = Object.getOwnPropertyDescriptor;
    Object.getOwnPropertyDescriptor = function(obj, prop) {
        if (obj === Navigator.prototype && prop === 'webdriver') {
            return {
                get: webdriverGetter,
                set: undefined,
                enumerable: true,
                configurable: true
            };
        }
        return originalGetDescriptor.apply(this, arguments);
    };
    maskedFunctions.set(Object.getOwnPropertyDescriptor, "function getOwnPropertyDescriptor() { [native code] }");

    Object.defineProperty(Navigator.prototype, 'webdriver', {
        get: webdriverGetter,
        configurable: true,
        enumerable: true
    });
    try {
        delete navigator.webdriver;
    } catch (e) {}

    mask(navigator, 'platform', 'Linux x86_64');
    mask(navigator, 'hardwareConcurrency', 8);
    mask(navigator, 'deviceMemory', 16);

    // 3. WebGL: Perfect Prototype Mock (NVIDIA RTX 3080)
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
        
        if (type === 'webgl' || type === 'webgl2' || type === 'experimental-webgl') {
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
            
            if (typeof WebGLRenderingContext !== 'undefined') {
                Object.setPrototypeOf(mockContext, WebGLRenderingContext.prototype);
            }
            
            return mockContext;
        }
        return realCtx;
    };
    maskedFunctions.set(HTMLCanvasElement.prototype.getContext, "function getContext() { [native code] }");

    // 4. Chrome runtime sterilization
    if (window.chrome && chrome.runtime) {
        const originalSendMessage = chrome.runtime.sendMessage;
        const maskedSendMessage = function() {
            if (arguments[0] && arguments[0].type === "PING") return;
            return originalSendMessage.apply(this, arguments);
        };
        chrome.runtime.sendMessage = maskedSendMessage;
        maskedFunctions.set(maskedSendMessage, "function sendMessage() { [native code] }");
    }

    // 5. Detect and hide CDC/Automation properties on window/document
    const cleanProps = ['cdc_ado8s0jhke7986_Array', 'cdc_ado8s0jhke7986_Promise', 'cdc_ado8s0jhke7986_Symbol'];
    cleanProps.forEach(p => {
        try { delete window[p]; } catch(e) {}
    });
})();
