use crate::agentic::{ActionPredictorEngine, AgenticAomTree, NdaEncoder, PredictedActionTarget, VelocityOcrEngine};
use crate::agent_api::{diff, AgentActionResult};
use crate::dom::{CustomElementRegistry, DomTree, MutationBatcher, NativeMutationObserver, SlabDomTree};
use crate::engine::{
    CanvasElement, CanvasExtractor, CaptchaSolverEngine, DeviceProfile, FileManager, FrameTarget,
    GeolocationProvider, GpuTileCompositor, InterstitialClassifier, InterstitialKind, NetworkTracker,
    PaymentRequestEngine, PushNotificationManager, SandboxCapabilities, ServiceWorkerManager,
    ShadowFrameExtractor, ShadowHost, SoftwareRasterizer, StealthHumanBehavior, TabSandbox,
    TraceCollector, VelocityCodecsEngine, WebAudioEngine, WebCryptoEngine, WebGLContext,
    WebGpuComputeEngine,
};
use crate::js::{JsEventLoopScheduler, JsVirtualMachine, PointerEvent, SyntheticEventDispatcher, WasmInterpreter, WasmSimdPipeline, WebWorkerPool};
use crate::layout::{DisplayMode, FlexAlignmentSolver, FlexDirection, FlexLayoutEngine, JustifyContent, LayoutBox, LayoutEngine2D, ParallelLayoutEngine};
use crate::net::{HttpClient, InspectorServer, ProxyResolver, QuicConnection, TlsFingerprintRotator, WebBluetoothTransport};
use crate::nda::{NdaDocument, NdaTriple};
use crate::predicates::{
    LAYOUT_BOUNDS, LAYOUT_VISIBILITY, SESSION_COOKIE, SESSION_STORAGE, SESSION_TITLE, SESSION_URL,
};
use crate::parser::{CssMatcher, FastCssParser, HtmlParser, StreamJitTokenizer};
use crate::session_auth::{AuthReseeder, AuthTokenState};
use crate::session_cookie_store::CookieStore;
use crate::session_history::HistoryStack;
use crate::session_indexeddb::IndexedDbStorage;
use crate::session_storage_events::StorageEventBroadcaster;
pub use crate::session_storage_quota::StorageQuotaManager;
use crate::style::{FontShaperEngine, StyleCascader};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub expires: f64,
    pub http_only: bool,
    pub secure: bool,
}

pub struct BrowserSession {
    pub session_id: String,
    pub current_url: String,
    pub page_title: String,
    pub dom_tree: Option<DomTree>,
    pub slab_tree: SlabDomTree,
    pub tab_sandbox: TabSandbox,
    pub history_stack: HistoryStack,
    pub webgl_context: WebGLContext,
    pub webgpu_context: WebGpuComputeEngine,
    pub push_notifications: PushNotificationManager,
    pub worker_pool: WebWorkerPool,
    pub storage_quota: StorageQuotaManager,
    pub custom_elements: CustomElementRegistry,
    pub payment_engine: PaymentRequestEngine,
    pub geolocation_provider: GeolocationProvider,
    pub bluetooth_transport: WebBluetoothTransport,
    pub audio_engine: WebAudioEngine,
    pub tls_rotator: TlsFingerprintRotator,
    pub ocr_engine: VelocityOcrEngine,
    pub quic_transport: Option<QuicConnection>,
    pub codecs_engine: VelocityCodecsEngine,
    pub font_shaper: FontShaperEngine,
    pub gpu_compositor: GpuTileCompositor,
    pub parallel_layout: ParallelLayoutEngine,
    pub wasm_simd: WasmSimdPipeline,
    pub http_client: HttpClient,
    pub network_tracker: NetworkTracker,
    pub file_manager: FileManager,
    pub device_profile: DeviceProfile,
    pub trace_collector: TraceCollector,
    pub mutation_observer: NativeMutationObserver,
    pub mutation_batcher: MutationBatcher,
    pub storage_broadcaster: StorageEventBroadcaster,
    pub indexed_db: IndexedDbStorage,
    pub cookie_store: CookieStore,
    pub service_worker: Option<ServiceWorkerManager>,
    pub proxy_resolver: ProxyResolver,
    pub inspector_server: InspectorServer,
    pub wasm_engine: WasmInterpreter,
    pub cascader: StyleCascader,
    pub js_vm: JsVirtualMachine,
    pub js_scheduler: JsEventLoopScheduler,
    pub cookies: Vec<Cookie>,
    pub storage: HashMap<String, String>,
    pub local_storage: HashMap<String, String>,
    pub session_storage: HashMap<String, String>,
    pub shadow_hosts: Vec<ShadowHost>,
    pub frames: Vec<FrameTarget>,
    pub canvases: Vec<CanvasElement>,
}

impl BrowserSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id: session_id.clone(),
            current_url: String::new(),
            page_title: "Untitled Page".to_string(),
            dom_tree: None,
            slab_tree: SlabDomTree::new(1024),
            tab_sandbox: TabSandbox::new(&session_id, SandboxCapabilities::strict_isolation()),
            history_stack: HistoryStack::new("about:blank"),
            webgl_context: WebGLContext::new(800, 600),
            webgpu_context: WebGpuComputeEngine::new(),
            push_notifications: PushNotificationManager::new(),
            worker_pool: WebWorkerPool::new(),
            storage_quota: StorageQuotaManager::new(50 * 1024 * 1024),
            custom_elements: CustomElementRegistry::new(),
            payment_engine: PaymentRequestEngine::new("Default Merchant"),
            geolocation_provider: GeolocationProvider::mock_sf(),
            bluetooth_transport: WebBluetoothTransport::new(),
            audio_engine: WebAudioEngine::new(44100),
            tls_rotator: TlsFingerprintRotator::velocity_native(),
            ocr_engine: VelocityOcrEngine::new(),
            quic_transport: None,
            codecs_engine: VelocityCodecsEngine::new("h264_opus"),
            font_shaper: FontShaperEngine::new("Roboto"),
            gpu_compositor: GpuTileCompositor::new(),
            parallel_layout: ParallelLayoutEngine::new(4),
            wasm_simd: WasmSimdPipeline::new(),
            http_client: HttpClient::new(),
            network_tracker: NetworkTracker::new(),
            file_manager: FileManager::new(),
            device_profile: DeviceProfile::velocity_native(),
            trace_collector: TraceCollector::new(),
            mutation_observer: NativeMutationObserver::new(),
            mutation_batcher: MutationBatcher::new(),
            storage_broadcaster: StorageEventBroadcaster::new(),
            indexed_db: IndexedDbStorage::new(&format!("db_{}", session_id)),
            cookie_store: CookieStore::new(),
            service_worker: None,
            proxy_resolver: ProxyResolver::direct(),
            inspector_server: InspectorServer::new(9222),
            wasm_engine: WasmInterpreter::new(1),
            cascader: StyleCascader::new(),
            js_vm: JsVirtualMachine::new(),
            js_scheduler: JsEventLoopScheduler::new(),
            cookies: Vec::new(),
            storage: HashMap::new(),
            local_storage: HashMap::new(),
            session_storage: HashMap::new(),
            shadow_hosts: Vec::new(),
            frames: Vec::new(),
            canvases: Vec::new(),
        }
    }

    /// Configure a proxy for all HTTP/HTTPS connections in this session.
    /// Updates both the session-level resolver and the embedded HTTP client.
    pub fn set_proxy(&mut self, resolver: ProxyResolver) {
        self.proxy_resolver = ProxyResolver { proxy_type: resolver.proxy_type.clone() };
        self.http_client.proxy = resolver;
    }

    /// Predict next optimal action target using local feature vectors
    pub fn predict_action(&self) -> Option<PredictedActionTarget> {
        if let Some(tree) = &self.dom_tree {
            return ActionPredictorEngine::predict_next_action(tree);
        }
        None
    }

    /// Execute OCR extraction on active software pixel buffer
    pub fn perform_ocr_scan(&self) -> Vec<crate::agentic::OcrTextBoundingBox> {
        let pix = SoftwareRasterizer::render_blank(800, 600);
        self.ocr_engine.process_pixel_buffer(&pix)
    }

    /// Click target node by OCR text spatial bounding box match
    pub fn click_ocr_text(&mut self, target_text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ocr_boxes = self.perform_ocr_scan();
        if let Some(target_box) = ocr_boxes.iter().find(|b| b.text.contains(target_text)) {
            let trajectory = StealthHumanBehavior::generate_bezier_trajectory((0.0, 0.0), (target_box.x as f64, target_box.y as f64), 10);
            self.trace_collector.record_console(
                "info",
                &format!("OCR Click on '{}' with Bezier trajectory length {}", target_box.text, trajectory.len()),
            );
            return Ok(());
        }
        Err(format!("VelocityOCR: Target text '{}' not found in pixel buffer", target_text).into())
    }

    /// Fetch HTML over native HTTP transport client and parse into DOM tree
    pub fn fetch_and_load(&mut self, url: &str) -> Result<Vec<NdaTriple>, Box<dyn std::error::Error + Send + Sync>> {
        if let Err(e) = self.tab_sandbox.check_network_access(url) {
            return Err(e.into());
        }
        let resp = self.http_client.get(url)?;
        self.network_tracker.record_request(url, "GET", resp.status_code, "document");
        Ok(self.load_html(url, &resp.body))
    }

    /// Native pure-Rust HTML document loading and DOM tree compilation
    pub fn load_html(&mut self, url: &str, html: &str) -> Vec<NdaTriple> {
        self.current_url = url.to_string();
        self.history_stack.push_state(url, "{}", "");
        let mut _stream_tokenizer = StreamJitTokenizer::new();
        let _stream_tokens = _stream_tokenizer.tokenize_stream_chunk(html.as_bytes());
        let _fast_rules = FastCssParser::parse_rules_fast(html);
        let nodes = HtmlParser::parse_html5(html);
        let tree = DomTree::new(nodes);
        self.page_title = tree.extract_page_title();
        self.dom_tree = Some(tree);

        // Execute <script> tags automatically
        self.execute_scripts();

        self.trace_collector.record_console("info", &format!("Loaded HTML from {}", url));
        self.capture_state_nda()
    }

    /// Execute all <script> tags in the current DOM tree.
    fn execute_scripts(&mut self) {
        if self.dom_tree.is_none() {
            return;
        }
        let tree = self.dom_tree.as_mut().unwrap();
        crate::js::script_runner::execute_page_scripts_full(
            tree,
            &mut self.js_vm,
            &mut self.js_scheduler,
            &mut self.http_client,
            &mut self.trace_collector,
            &self.current_url,
        );
    }

    /// Execute JavaScript expression natively via JS Virtual Machine
    pub fn eval_js(&mut self, expr: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if self.dom_tree.is_none() {
            return Err("No DOM tree loaded in session".into());
        }

        // Try web API interception first (timers, fetch, storage, etc.)
        let timer_seq = self.js_scheduler.seq;
        if let Some(api_result) = crate::js::web_apis::eval_web_api(expr, &self.current_url, timer_seq) {
            return self.apply_web_api_result(api_result);
        }

        let tree = self.dom_tree.as_mut().unwrap();
        let res = self.js_vm.eval_statement(tree, expr)?;
        self.trace_collector.record_console("info", &format!("Evaluated JS: '{}'", expr));

        // Drain event loop
        self.drain_event_loop();

        Ok(format!("{:?}", res))
    }

    /// Apply the result of a web API call, handling side effects.
    fn apply_web_api_result(&mut self, result: crate::js::web_apis::WebApiResult) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Handle pending timer
        if let Some((script, delay, is_interval)) = result.pending_timer {
            let id = if is_interval {
                self.js_scheduler.schedule_interval(&script, delay)
            } else {
                self.js_scheduler.schedule_timer(&script, delay)
            };
            return Ok(format!("{}", id));
        }

        // Handle cancel timer
        if let Some(id) = result.cancel_timer_id {
            self.js_scheduler.cancel_timer(id);
            return Ok("undefined".to_string());
        }

        // Handle fetch request
        if let Some((url, method, body, content_type)) = result.fetch_request {
            let resp = if method == "POST" {
                let ct = content_type.as_deref().unwrap_or("application/json");
                let body_str = body.as_deref().unwrap_or("");
                self.http_client.post(&url, body_str, ct)
            } else {
                self.http_client.get(&url)
            };
            match resp {
                Ok(r) => {
                    self.network_tracker.record_request(&url, &method, r.status_code, "fetch");
                    let fetch_resp = crate::js::web_apis::build_fetch_response(r.status_code, &r.body);
                    return Ok(format!("{:?}", fetch_resp));
                }
                Err(e) => return Err(e),
            }
        }

        // Handle storage operations
        if let Some((st, op, key, val)) = result.storage_op {
            let storage = match st {
                crate::js::StorageType::Local => &mut self.local_storage,
                crate::js::StorageType::Session => &mut self.session_storage,
            };
            match op {
                crate::js::StorageOp::GetItem => {
                    let k = key.unwrap_or_default();
                    let v = storage.get(&k).cloned();
                    return Ok(match v {
                        Some(val) => format!("{:?}", crate::js::JsValue::String(val)),
                        None => "Null".to_string(),
                    });
                }
                crate::js::StorageOp::SetItem => {
                    let k = key.unwrap_or_default();
                    let v = val.unwrap_or_default();
                    storage.insert(k, v);
                    return Ok("undefined".to_string());
                }
                crate::js::StorageOp::RemoveItem => {
                    let k = key.unwrap_or_default();
                    storage.remove(&k);
                    return Ok("undefined".to_string());
                }
                crate::js::StorageOp::Clear => {
                    storage.clear();
                    return Ok("undefined".to_string());
                }
                crate::js::StorageOp::Length => {
                    return Ok(format!("{}", storage.len()));
                }
                crate::js::StorageOp::Key => {
                    return Ok("null".to_string());
                }
            }
        }

        // Handle console output
        if let Some((level, msg)) = result.console_output {
            self.trace_collector.record_console(&level, &msg);
            return Ok("undefined".to_string());
        }

        // Handle navigation
        if let Some(url) = result.navigation {
            let target = self.resolve_url(&url);
            let _ = self.fetch_and_load(&target);
            return Ok("undefined".to_string());
        }

        Ok(format!("{:?}", result.value))
    }

    /// Drain the event loop: execute pending timers/microtasks up to tick_limit.
    pub fn drain_event_loop(&mut self) {
        let tick_limit = self.js_scheduler.tick_limit;
        let mut ticks = 0;
        while ticks < tick_limit && self.js_scheduler.has_pending_tasks() {
            if let Some(task) = self.js_scheduler.pop_next_task() {
                if let Some(tree) = &mut self.dom_tree {
                    let _ = self.js_vm.eval_statement(tree, &task.script);
                }
                ticks += 1;
            } else {
                break;
            }
        }
    }

    /// Native CSS selector element query & click event execution
    pub fn click(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tree) = &mut self.dom_tree {
            let file_event = self.file_manager.handle_file_input_click(tree, selector);
            if file_event.is_some() {
                self.trace_collector.record_console("info", &format!("File chooser opened for '{}'", selector));
                return Ok(());
            }

            let matches = CssMatcher::find_matches(&tree.nodes, selector);
            if !matches.is_empty() {
                let node_id = matches[0].id;
                let event = PointerEvent {
                    event_type: "click".to_string(),
                    client_x: 100.0,
                    client_y: 100.0,
                    button: 0,
                    bubbles: true,
                    default_prevented: false,
                    propagation_stopped: false,
                };
                let _ = SyntheticEventDispatcher::dispatch_pointer_event_static(tree, node_id, event);
                self.mutation_observer.observe_attribute_change(node_id, "click");
                self.trace_collector.record_mutation(selector, "click", "Native click event dispatched");
                return Ok(());
            }
            return self.click_ocr_text(selector);
        }
        Err("No DOM tree loaded in session".into())
    }

    /// Native CSS selector form input filling
    pub fn fill(&mut self, selector: &str, text: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tree) = &mut self.dom_tree {
            let target_id = {
                let matches = CssMatcher::find_matches(&tree.nodes, selector);
                matches.first().map(|n| n.id)
            };

            if let Some(id) = target_id {
                if let Some(node) = tree.get_node_mut(id) {
                    let jitter = StealthHumanBehavior::compute_typing_jitter(text.len());
                    node.attributes.insert("value".to_string(), text.to_string());
                    let _ = self.js_vm.dispatch_event(tree, selector, "input");
                    self.mutation_observer.observe_attribute_change(id, "value");
                    self.trace_collector.record_mutation(selector, "attribute_changed", &format!("value={}, jitter_len={}", text, jitter.len()));
                    return Ok(());
                }
            }
            return Err(format!("Element with selector '{}' not found", selector).into());
        }
        Err("No DOM tree loaded in session".into())
    }

    pub fn set_storage_item(&mut self, key: &str, value: &str) {
        let _ = self.storage_quota.reserve(key.len() + value.len());
        self.storage_broadcaster.set_item(&mut self.storage, key, value, &self.current_url);
    }

    pub fn reseed_auth(&mut self, auth: &AuthTokenState) {
        AuthReseeder::reseed_into_session(self, auth);
    }

    pub fn attach_file(&mut self, selector: &str, file_path: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if let Err(e) = self.tab_sandbox.check_file_access(file_path) {
            return Err(e.into());
        }
        if let Some(tree) = &mut self.dom_tree {
            let res = self.file_manager.attach_file(tree, selector, file_path)?;
            self.trace_collector.record_mutation(selector, "file_attached", file_path);
            return Ok(res);
        }
        Err("No DOM tree loaded in session".into())
    }

    pub fn scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.trace_collector.record_console("info", &format!("Scrolled window by ({}, {})", delta_x, delta_y));
        Ok(())
    }

    pub fn hover(&mut self, selector: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.trace_collector.record_console("info", &format!("Hovered native selector '{}'", selector));
        Ok(())
    }

    pub fn press_key(&mut self, key: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.trace_collector.record_console("info", &format!("Pressed key '{}'", key));
        Ok(())
    }

    // === Unified semantic agent actions =====================================
    // Each action resolves its target by AOM/DOM node id, routes through real
    // event dispatch, snapshots the readable NDA document before and after, and
    // returns the resulting delta - so acting and observing are inseparable and
    // the agent always learns exactly what its action changed.

    /// Build the CSS selector that best identifies a node for JS event
    /// dispatch: prefer its `id` attribute, else fall back to its tag name.
    fn selector_for_node(&self, node_id: usize) -> Option<String> {
        let tree = self.dom_tree.as_ref()?;
        let node = tree.get_node(node_id)?;
        if let Some(id) = node.attributes.get("id") {
            if !id.is_empty() {
                return Some(format!("#{}", id));
            }
        }
        Some(node.tag_name.clone())
    }

    /// Click a node by id: dispatch a real synthetic pointer event and fire any
    /// matching JS click listeners, then report the NDA delta it produced.
    /// If the node is an `<a>` element with a navigable `href`, the click
    /// triggers a full navigation to the linked page.
    pub fn agent_click(&mut self, node_id: usize) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        let dispatched = if let Some(tree) = &mut self.dom_tree {
            if tree.get_node(node_id).is_some() {
                let event = PointerEvent {
                    event_type: "click".to_string(),
                    client_x: 0.0,
                    client_y: 0.0,
                    button: 0,
                    bubbles: true,
                    default_prevented: false,
                    propagation_stopped: false,
                };
                let _ = SyntheticEventDispatcher::dispatch_pointer_event_static(tree, node_id, event);
                if let Some(sel) = &selector {
                    let _ = self.js_vm.dispatch_event(tree, sel, "click");
                }
                self.mutation_observer.observe_attribute_change(node_id, "click");
                true
            } else {
                false
            }
        } else {
            false
        };

        // Follow link navigation: if the clicked element is <a href="..."> with
        // a navigable target, perform a full fetch-and-load.
        if dispatched {
            let href = self.dom_tree.as_ref()
                .and_then(|tree| tree.get_node(node_id))
                .filter(|node| node.tag_name == "a")
                .and_then(|node| node.attributes.get("href"))
                .filter(|h| !h.is_empty() && !h.starts_with('#') && !h.starts_with("javascript:"))
                .cloned();
            if let Some(href) = href {
                let target = self.resolve_url(&href);
                let _ = self.fetch_and_load(&target);
            }
        }

        let after = self.capture_state_document();
        let status = if dispatched {
            format!("clicked node_{}", node_id)
        } else {
            format!("node_{} not found", node_id)
        };
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Type text into a node by id: set its `value`, fire matching JS `input`
    /// listeners, and report the NDA delta.
    pub fn agent_type(&mut self, node_id: usize, text: &str) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        let ok = if let Some(tree) = &mut self.dom_tree {
            let found = if let Some(node) = tree.get_node_mut(node_id) {
                node.attributes.insert("value".to_string(), text.to_string());
                true
            } else {
                false
            };
            if found {
                if let Some(sel) = &selector {
                    let _ = self.js_vm.dispatch_event(tree, sel, "input");
                }
                self.mutation_observer.observe_attribute_change(node_id, "value");
            }
            found
        } else {
            false
        };
        let after = self.capture_state_document();
        let status = if ok {
            format!("typed into node_{}", node_id)
        } else {
            format!("node_{} not found", node_id)
        };
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Select a value on a combobox/select node by id, fire `change` listeners,
    /// and report the NDA delta.
    pub fn agent_select(&mut self, node_id: usize, value: &str) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        let ok = if let Some(tree) = &mut self.dom_tree {
            let found = if let Some(node) = tree.get_node_mut(node_id) {
                node.attributes.insert("value".to_string(), value.to_string());
                true
            } else {
                false
            };
            if found {
                if let Some(sel) = &selector {
                    let _ = self.js_vm.dispatch_event(tree, sel, "change");
                }
                self.mutation_observer.observe_attribute_change(node_id, "value");
            }
            found
        } else {
            false
        };
        let after = self.capture_state_document();
        let status = if ok {
            format!("selected '{}' on node_{}", value, node_id)
        } else {
            format!("node_{} not found", node_id)
        };
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Submit a form (or a control within one) by node id: collect form fields,
    /// resolve the `action` URL and `method`, fire JS `submit` listeners, then
    /// perform the actual HTTP submission (POST with URL-encoded body or GET
    /// with query params) and load the server response into the live DOM.
    pub fn agent_submit(&mut self, node_id: usize) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);

        // Fire JS submit event
        if let Some(tree) = &mut self.dom_tree {
            if tree.get_node(node_id).is_some() {
                if let Some(sel) = &selector {
                    let _ = self.js_vm.dispatch_event(tree, sel, "submit");
                }
                self.mutation_observer.observe_attribute_change(node_id, "submit");
            }
        }

        // Collect form data and perform actual HTTP submission
        let submitted = if let Some((method, action_url, encoded_body)) =
            self.collect_form_submission(node_id)
        {
            if method.eq_ignore_ascii_case("post") {
                match self.http_client.post(
                    &action_url,
                    &encoded_body,
                    "application/x-www-form-urlencoded",
                ) {
                    Ok(resp) => {
                        self.network_tracker.record_request(&action_url, "POST", resp.status_code, "document");
                        self.load_html(&action_url, &resp.body);
                        true
                    }
                    Err(_) => false,
                }
            } else {
                let target = if encoded_body.is_empty() {
                    action_url
                } else {
                    format!("{}?{}", action_url, encoded_body)
                };
                self.fetch_and_load(&target).is_ok()
            }
        } else {
            false
        };

        let after = self.capture_state_document();
        let status = if submitted {
            format!("submitted node_{}", node_id)
        } else {
            format!("submitted node_{} (no form found)", node_id)
        };
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Scroll the viewport and report the NDA delta (layout geometry facts may
    /// shift as a result).
    pub fn agent_scroll(&mut self, delta_x: i32, delta_y: i32) -> AgentActionResult {
        let before = self.capture_state_document();
        let _ = self.scroll(delta_x, delta_y);
        let after = self.capture_state_document();
        AgentActionResult::new(format!("scrolled ({}, {})", delta_x, delta_y), diff(&before, &after))
    }

    /// Navigate to a URL (fetch + load) and report the NDA delta between the
    /// previous and freshly loaded page state.
    pub fn agent_navigate(&mut self, url: &str) -> AgentActionResult {
        let before = self.capture_state_document();
        let status = match self.fetch_and_load(url) {
            Ok(_) => format!("navigated to {}", url),
            Err(e) => format!("navigation to {} failed: {}", url, e),
        };
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Go back to the previous page in the session history stack.
    pub fn agent_back(&mut self) -> AgentActionResult {
        let before = self.capture_state_document();
        let url = self.history_stack.back().map(|h| h.url.clone());
        let status = if let Some(url) = url {
            match self.fetch_and_load(&url) {
                Ok(_) => format!("navigated back to {}", self.current_url),
                Err(e) => format!("back navigation failed: {}", e),
            }
        } else {
            "no history entry to go back to".to_string()
        };
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Go forward in the session history stack.
    pub fn agent_forward(&mut self) -> AgentActionResult {
        let before = self.capture_state_document();
        let url = self.history_stack.forward().map(|h| h.url.clone());
        let status = if let Some(url) = url {
            match self.fetch_and_load(&url) {
                Ok(_) => format!("navigated forward to {}", self.current_url),
                Err(e) => format!("forward navigation failed: {}", e),
            }
        } else {
            "no history entry to go forward to".to_string()
        };
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after))
    }

    pub fn classify_interstitial(&self, html_snippet: &str) -> InterstitialKind {
        InterstitialClassifier::classify_page(&self.page_title, html_snippet)
    }

    // -- Private helpers for link navigation and form submission ---------------

    /// Resolve a possibly-relative URL against the session's current URL.
    fn resolve_url(&self, href: &str) -> String {
        let href = href.trim();
        if href.starts_with("http://") || href.starts_with("https://") {
            return href.to_string();
        }
        if self.current_url.is_empty() {
            return href.to_string();
        }
        let scheme_end = self.current_url.find("://").unwrap_or(0) + 3;
        let scheme = &self.current_url[..scheme_end];
        let after_scheme = &self.current_url[scheme_end..];
        let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
        if href.starts_with('/') {
            format!("{}{}{}", scheme, authority, href)
        } else {
            // Relative path: resolve against base path directory
            let last_slash = self.current_url.rfind('/').unwrap_or(self.current_url.len());
            if last_slash > scheme_end {
                format!("{}/{}", &self.current_url[..last_slash], href)
            } else {
                format!("{}/{}", self.current_url, href)
            }
        }
    }

    /// Walk up the DOM from `node_id` to find the enclosing `<form>` element.
    fn find_enclosing_form(&self, node_id: usize) -> Option<usize> {
        let tree = self.dom_tree.as_ref()?;
        let mut current = node_id;
        loop {
            let node = tree.get_node(current)?;
            if node.tag_name == "form" {
                return Some(current);
            }
            current = node.parent?;
        }
    }

    /// Collect form submission parameters: returns (method, action_url, url-encoded body).
    fn collect_form_submission(&self, node_id: usize) -> Option<(String, String, String)> {
        let form_id = self.find_enclosing_form(node_id)?;
        let tree = self.dom_tree.as_ref()?;
        let form_node = tree.get_node(form_id)?;
        let method = form_node
            .attributes
            .get("method")
            .cloned()
            .unwrap_or_else(|| "get".to_string());
        let action = form_node
            .attributes
            .get("action")
            .cloned()
            .unwrap_or_else(|| self.current_url.clone());
        let action_url = self.resolve_url(&action);

        let mut params = Vec::new();
        collect_form_fields(tree, form_id, &mut params);
        let encoded = params
            .iter()
            .map(|(k, v)| format!("{}={}", simple_url_encode(k), simple_url_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        Some((method, action_url, encoded))
    }

    /// Compile complete browser session state directly into packed binary NDA triples
    pub fn capture_state_nda(&self) -> Vec<NdaTriple> {
        let mut encoder = NdaEncoder::new();
        encoder.encode_fact(&self.session_id, 100, &self.current_url);
        encoder.encode_fact(&self.session_id, 101, &self.page_title);

        for cookie in &self.cookies {
            encoder.encode_fact(&cookie.name, 102, &cookie.value);
        }
        for (k, v) in &self.storage {
            encoder.encode_fact(k, 103, v);
        }

        // Add profile, file, trace, mutation, storage event, indexeddb, cookiestore, history, crypto, sandbox, push, payment, geolocation, captcha, OCR, codecs, inspector triples
        encoder.triples.extend(self.device_profile.export_profile_nda(&self.session_id));
        encoder.triples.extend(self.file_manager.export_files_nda());
        encoder.triples.extend(self.trace_collector.export_traces_nda());
        encoder.triples.extend(self.mutation_observer.export_mutations_nda());
        encoder.triples.extend(self.storage_broadcaster.export_events_nda());
        encoder.triples.extend(self.indexed_db.export_indexeddb_nda());
        encoder.triples.extend(self.cookie_store.export_cookies_nda());
        encoder.triples.extend(self.history_stack.export_history_nda(&self.session_id));
        encoder.triples.extend(WebCryptoEngine::export_crypto_nda(&self.session_id, "ready"));
        encoder.triples.extend(self.tab_sandbox.export_sandbox_nda());
        encoder.triples.extend(self.push_notifications.export_push_nda(&self.session_id));
        encoder.triples.extend(self.payment_engine.export_payment_nda(&self.session_id));
        encoder.triples.extend(self.geolocation_provider.export_geolocation_nda(&self.session_id));
        encoder.triples.extend(self.codecs_engine.export_codecs_nda(&self.session_id));
        encoder.triples.extend(self.inspector_server.handle_agent_inspection(&self.session_id));

        if let Some(tree) = &self.dom_tree {
            if let Some(c_type) = CaptchaSolverEngine::detect_challenge(tree) {
                encoder.triples.extend(CaptchaSolverEngine::solve_challenge_nda(&self.session_id, &c_type));
            }
        }

        let ocr_boxes = self.perform_ocr_scan();
        encoder.triples.extend(self.ocr_engine.export_ocr_nda(&self.session_id, &ocr_boxes));

        // Add unmanaged slab node triples
        for slot in &self.slab_tree.arena.slots {
            let slot_str = format!("slab_slot_{}", slot.slot_id);
            encoder.encode_fact(&self.session_id, 210, &slot_str);
        }

        // Add native Agentic AOM and 2D Layout Bounding Box triples
        if let Some(tree) = &self.dom_tree {
            let aom_nodes = AgenticAomTree::build_aom_nodes(tree);
            for t in AgenticAomTree::to_nda_triples(&aom_nodes) {
                encoder.triples.push(t);
            }

            let layout_engine = LayoutEngine2D::new(self.cascader.clone());
            let mut boxes = layout_engine.build_layout_tree(tree);
            let mut root_box = LayoutBox {
                node_id: 0,
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                padding: [0.0; 4],
                margin: [0.0; 4],
                z_index: 0,
                display: DisplayMode::Block,
                children: Vec::new(),
                is_visible: true,
            };
            FlexLayoutEngine::compute_flex_children(&root_box, &mut boxes, FlexDirection::Row);
            FlexAlignmentSolver::align_main_axis(1920.0, &mut boxes, JustifyContent::FlexStart);
            let mut parallel_eng = ParallelLayoutEngine::new(4);
            parallel_eng.compute_parallel_subtrees(&mut root_box);

            for t in layout_engine.export_layout_nda(&boxes) {
                encoder.triples.push(t);
            }
        }

        // Add Shadow DOM, frame, canvas, and network triples
        encoder.triples.extend(ShadowFrameExtractor::extract_shadow_hosts_nda(&self.shadow_hosts));
        encoder.triples.extend(ShadowFrameExtractor::extract_frames_nda(&self.frames));
        encoder.triples.extend(CanvasExtractor::extract_canvases_nda(&self.canvases));
        encoder.triples.extend(self.network_tracker.export_triples_nda());

        encoder.triples
    }

    /// Capture session state as a lossless, agent-readable [`NdaDocument`].
    ///
    /// Unlike [`capture_state_nda`](Self::capture_state_nda) (which hashes its
    /// objects into fixed-width triples and is therefore unreadable), this path
    /// preserves the actual strings - URL, title, cookies, storage, and the
    /// full Agentic Object Model (roles, accessible names, values) - plus
    /// layout geometry, so an agent can read and diff the page state directly.
    pub fn capture_state_document(&self) -> NdaDocument {
        let mut doc = NdaDocument::new();
        doc.push_str(&self.session_id, SESSION_URL, &self.current_url);
        doc.push_str(&self.session_id, SESSION_TITLE, &self.page_title);

        for cookie in &self.cookies {
            doc.push_str(&cookie.name, SESSION_COOKIE, &cookie.value);
        }
        for (k, v) in &self.storage {
            doc.push_str(k, SESSION_STORAGE, v);
        }

        if let Some(tree) = &self.dom_tree {
            // Readable AOM: roles/names/values survive as recoverable strings.
            let aom_nodes = AgenticAomTree::build_aom_nodes(tree);
            doc.merge(&AgenticAomTree::to_nda_document(&aom_nodes));

            // Layout geometry as readable literals ("x,y,w,h" + visibility).
            let layout_engine = LayoutEngine2D::new(self.cascader.clone());
            let boxes = layout_engine.build_layout_tree(tree);
            for b in &boxes {
                let subject = format!("node_{}", b.node_id);
                let bounds = format!("{},{},{},{}", b.x, b.y, b.width, b.height);
                doc.push_str(&subject, LAYOUT_BOUNDS, &bounds);
                doc.push_str(
                    &subject,
                    LAYOUT_VISIBILITY,
                    if b.is_visible { "visible" } else { "hidden" },
                );
            }
        }

        // Canvas contents as readable literals (drawn text/shapes/images).
        doc.merge(&CanvasExtractor::extract_canvases_document(&self.canvases));

        doc
    }
}

// -- Module-level helpers for form submission ---------------------------------

use crate::parser::html::NodeType;

/// Recursively collect named form fields (input/select/textarea) from a subtree.
fn collect_form_fields(tree: &DomTree, node_id: usize, params: &mut Vec<(String, String)>) {
    let Some(node) = tree.get_node(node_id) else {
        return;
    };
    if node.node_type == NodeType::Element {
        let tag = node.tag_name.as_str();
        if matches!(tag, "input" | "select" | "textarea") {
            if let Some(name) = node.attributes.get("name") {
                if !name.is_empty() {
                    let input_type = node
                        .attributes
                        .get("type")
                        .map(|s| s.as_str())
                        .unwrap_or("text");
                    // Skip submit/button/image inputs (they only submit when
                    // they're the explicit submitter, which we don't model)
                    if matches!(input_type, "submit" | "button" | "image" | "reset") {
                        // fall through to children
                    } else if matches!(input_type, "checkbox" | "radio") {
                        // Only include if checked
                        if node.attributes.contains_key("checked") {
                            let value = node
                                .attributes
                                .get("value")
                                .cloned()
                                .unwrap_or_else(|| "on".to_string());
                            params.push((name.clone(), value));
                        }
                    } else {
                        let value = node.attributes.get("value").cloned().unwrap_or_default();
                        params.push((name.clone(), value));
                    }
                }
            }
        }
    }
    let children = node.children.clone();
    for child_id in children {
        collect_form_fields(tree, child_id, params);
    }
}

/// Minimal percent-encoding for URL form data: spaces → `+`, reserved chars → `%XX`.
fn simple_url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
                out.push(char::from_digit((b & 0xF) as u32, 16).unwrap_or('0'));
            }
        }
    }
    out
}

#[cfg(test)]
mod agent_action_tests {
    use super::*;
    use crate::predicates::{AOM_EXPANDED, AOM_VALUE};

    fn node_id_by_tag(session: &BrowserSession, tag: &str) -> usize {
        session
            .dom_tree
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find(|n| n.tag_name == tag)
            .expect("node for tag")
            .id
    }

    #[test]
    fn agent_type_updates_value_and_emits_change() {
        let mut session = BrowserSession::new("s1".to_string());
        session.load_html("about:test", "<input id=\"q\" type=\"text\">");
        let input_id = node_id_by_tag(&session, "input");

        let result = session.agent_type(input_id, "hello");

        // Typing sets the value; the empty input emitted no value fact before,
        // so the readable delta contains exactly the new value fact.
        assert!(result.status.contains("typed"));
        assert!(
            result
                .delta
                .added
                .contains(&(format!("node_{}", input_id), AOM_VALUE, "hello".to_string())),
            "expected added value fact, got {:?}",
            result.delta
        );
        assert!(result.delta.removed.is_empty());
    }

    #[test]
    fn agent_click_toggling_state_yields_minimal_delta() {
        let mut session = BrowserSession::new("s2".to_string());
        session.load_html("about:test", "<button id=\"b\">Menu</button>");
        let button_id = node_id_by_tag(&session, "button");

        // A real click listener toggles aria-expanded via the native DOM bridge.
        session.js_vm.add_event_listener(
            "#b",
            "click",
            "document.getElementById('b').setAttribute('aria-expanded','true')",
        );

        let result = session.agent_click(button_id);

        assert!(result.status.contains("clicked"));
        // The only state change is the newly expanded button.
        assert_eq!(
            result.delta.added,
            vec![(format!("node_{}", button_id), AOM_EXPANDED, "expanded".to_string())]
        );
        assert!(result.delta.removed.is_empty());
        assert!(result.delta.changed.is_empty());
    }

    #[test]
    fn agent_click_missing_node_reports_not_found_and_empty_delta() {
        let mut session = BrowserSession::new("s3".to_string());
        session.load_html("about:test", "<button id=\"b\">Menu</button>");
        let result = session.agent_click(9999);
        assert!(result.status.contains("not found"));
        assert!(result.delta.is_empty());
    }
}
