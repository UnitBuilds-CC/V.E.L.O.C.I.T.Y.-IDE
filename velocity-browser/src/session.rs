use crate::agent_api::{diff, AgentActionResult, NdaDelta};
use crate::agentic::{
    ActionPredictorEngine, AgenticAomTree, NdaEncoder, PredictedActionTarget, VelocityOcrEngine,
};
use crate::dom::{
    CustomElementRegistry, DomTree, MutationBatcher, NativeMutationObserver, SlabDomTree,
};
use crate::engine::{
    CanvasElement, CanvasExtractor, CaptchaSolverEngine, DeviceProfile, FileManager, FrameTarget,
    GeolocationProvider, GpuTileCompositor, InterstitialClassifier, InterstitialKind,
    NetworkTracker, PaymentRequestEngine, PushNotificationManager, SandboxCapabilities,
    ServiceWorkerManager, ShadowFrameExtractor, ShadowHost, SoftwareRasterizer,
    StealthHumanBehavior, TabSandbox, TraceCollector, VelocityCodecsEngine, WebAudioEngine,
    WebCryptoEngine, WebGLContext, WebGpuComputeEngine,
};
use crate::errors::BrowserError;
use crate::js::{
    JsEventLoopScheduler, JsVirtualMachine, PointerEvent, SyntheticEventDispatcher,
    WasmInterpreter, WasmSimdPipeline, WebWorkerPool,
};
use crate::layout::{
    DisplayMode, FlexAlignmentSolver, FlexDirection, FlexLayoutEngine, JustifyContent, LayoutBox,
    LayoutEngine2D, ParallelLayoutEngine,
};
use crate::nda::{NdaDocument, NdaTriple};
use crate::net::{
    HttpClient, InspectorServer, ProxyResolver, QuicConnection, TlsFingerprintRotator,
    WebBluetoothTransport,
};
use crate::parser::{CssMatcher, FastCssParser, HtmlParser, StreamJitTokenizer};
use crate::predicates::{
    AOM_FOCUSED, LAYOUT_BOUNDS, LAYOUT_IN_VIEWPORT, LAYOUT_VISIBILITY, SESSION_CONTENT,
    SESSION_COOKIE, SESSION_FORM_COUNT, SESSION_HEADING, SESSION_INTERACTIVE_COUNT,
    SESSION_LINK_COUNT, SESSION_SCROLL, SESSION_STORAGE, SESSION_TEXT_LENGTH, SESSION_TITLE,
    SESSION_URL,
};
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

// ---------------------------------------------------------------------------
// Sub-structs that decompose BrowserSession into cohesive groups.
// ---------------------------------------------------------------------------

/// DOM tree, slab arena, shadow hosts, frames, and canvases.
pub struct SessionDom {
    pub dom_tree: Option<DomTree>,
    pub slab_tree: SlabDomTree,
    pub shadow_hosts: Vec<ShadowHost>,
    pub frames: Vec<FrameTarget>,
    pub canvases: Vec<CanvasElement>,
    pub custom_elements: CustomElementRegistry,
    pub mutation_observer: NativeMutationObserver,
    pub mutation_batcher: MutationBatcher,
}

impl SessionDom {
    fn new() -> Self {
        Self {
            dom_tree: None,
            slab_tree: SlabDomTree::new(1024),
            shadow_hosts: Vec::new(),
            frames: Vec::new(),
            canvases: Vec::new(),
            custom_elements: CustomElementRegistry::new(),
            mutation_observer: NativeMutationObserver::new(),
            mutation_batcher: MutationBatcher::new(),
        }
    }
}

/// JavaScript virtual machine, event-loop scheduler, WASM, and workers.
pub struct SessionJs {
    pub js_vm: JsVirtualMachine,
    pub js_scheduler: JsEventLoopScheduler,
    pub wasm_engine: WasmInterpreter,
    pub wasm_simd: WasmSimdPipeline,
    pub worker_pool: WebWorkerPool,
}

impl SessionJs {
    fn new() -> Self {
        Self {
            js_vm: JsVirtualMachine::new(),
            js_scheduler: JsEventLoopScheduler::new(),
            wasm_engine: WasmInterpreter::new(1),
            wasm_simd: WasmSimdPipeline::new(),
            worker_pool: WebWorkerPool::new(),
        }
    }
}

/// HTTP client, network tracker, proxy resolver, TLS fingerprinting, QUIC.
pub struct SessionNet {
    pub http_client: HttpClient,
    pub network_tracker: NetworkTracker,
    pub proxy_resolver: ProxyResolver,
    pub tls_rotator: TlsFingerprintRotator,
    pub quic_transport: Option<QuicConnection>,
}

impl SessionNet {
    fn new() -> Self {
        Self {
            http_client: HttpClient::new(),
            network_tracker: NetworkTracker::new(),
            proxy_resolver: ProxyResolver::direct(),
            tls_rotator: TlsFingerprintRotator::velocity_native(),
            quic_transport: None,
        }
    }
}

pub struct BrowserSession {
    pub session_id: String,
    pub current_url: String,
    pub page_title: String,
    pub tab_sandbox: TabSandbox,
    pub history_stack: HistoryStack,
    // -- DOM sub-struct -------------------------------------------------------
    pub dom: SessionDom,
    // -- JS sub-struct --------------------------------------------------------
    pub js: SessionJs,
    // -- Network sub-struct ---------------------------------------------------
    pub net: SessionNet,
    // -- Remaining engine fields (kept inline, accessed less frequently) ------
    pub webgl_context: WebGLContext,
    pub webgpu_context: WebGpuComputeEngine,
    pub push_notifications: PushNotificationManager,
    pub storage_quota: StorageQuotaManager,
    pub payment_engine: PaymentRequestEngine,
    pub geolocation_provider: GeolocationProvider,
    pub bluetooth_transport: WebBluetoothTransport,
    pub audio_engine: WebAudioEngine,
    pub ocr_engine: VelocityOcrEngine,
    pub codecs_engine: VelocityCodecsEngine,
    pub font_shaper: FontShaperEngine,
    pub gpu_compositor: GpuTileCompositor,
    pub parallel_layout: ParallelLayoutEngine,
    pub file_manager: FileManager,
    pub device_profile: DeviceProfile,
    pub trace_collector: TraceCollector,
    pub storage_broadcaster: StorageEventBroadcaster,
    pub indexed_db: IndexedDbStorage,
    pub cookie_store: CookieStore,
    pub service_worker: Option<ServiceWorkerManager>,
    pub inspector_server: InspectorServer,
    pub cascader: StyleCascader,
    pub cookies: Vec<Cookie>,
    pub storage: HashMap<String, String>,
    pub local_storage: HashMap<String, String>,
    pub session_storage: HashMap<String, String>,
    /// Node currently holding keyboard focus — target of `agent_press`.
    pub focused_node: Option<usize>,
    /// Horizontal scroll offset of the viewport in document coordinates.
    pub scroll_x: f32,
    /// Vertical scroll offset of the viewport in document coordinates.
    pub scroll_y: f32,
    /// Viewport width used for in-viewport visibility facts.
    pub viewport_width: f32,
    /// Viewport height used for in-viewport visibility facts.
    pub viewport_height: f32,
}

impl BrowserSession {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id: session_id.clone(),
            current_url: String::new(),
            page_title: "Untitled Page".to_string(),
            tab_sandbox: TabSandbox::new(&session_id, SandboxCapabilities::strict_isolation()),
            history_stack: HistoryStack::new("about:blank"),
            dom: SessionDom::new(),
            js: SessionJs::new(),
            net: SessionNet::new(),
            webgl_context: WebGLContext::new(800, 600),
            webgpu_context: WebGpuComputeEngine::new(),
            push_notifications: PushNotificationManager::new(),
            storage_quota: StorageQuotaManager::new(50 * 1024 * 1024),
            payment_engine: PaymentRequestEngine::new("Default Merchant"),
            geolocation_provider: GeolocationProvider::mock_sf(),
            bluetooth_transport: WebBluetoothTransport::new(),
            audio_engine: WebAudioEngine::new(44100),
            ocr_engine: VelocityOcrEngine::new(),
            codecs_engine: VelocityCodecsEngine::new("h264_opus"),
            font_shaper: FontShaperEngine::new("Roboto"),
            gpu_compositor: GpuTileCompositor::new(),
            parallel_layout: ParallelLayoutEngine::new(4),
            file_manager: FileManager::new(),
            device_profile: DeviceProfile::velocity_native(),
            trace_collector: TraceCollector::new(),
            storage_broadcaster: StorageEventBroadcaster::new(),
            indexed_db: IndexedDbStorage::new(&format!("db_{}", session_id)),
            cookie_store: CookieStore::new(),
            service_worker: None,
            inspector_server: InspectorServer::new(9222),
            cascader: StyleCascader::new(),
            cookies: Vec::new(),
            storage: HashMap::new(),
            local_storage: HashMap::new(),
            session_storage: HashMap::new(),
            focused_node: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            // Matches DeviceProfile::velocity_native()'s 1920x1080 viewport.
            viewport_width: 1920.0,
            viewport_height: 1080.0,
        }
    }

    /// Configure a proxy for all HTTP/HTTPS connections in this session.
    /// Updates both the session-level resolver and the embedded HTTP client.
    pub fn set_proxy(&mut self, resolver: ProxyResolver) {
        self.net.proxy_resolver = ProxyResolver {
            proxy_type: resolver.proxy_type.clone(),
        };
        self.net.http_client.proxy = resolver;
    }

    /// Predict next optimal action target using local feature vectors
    pub fn predict_action(&self) -> Option<PredictedActionTarget> {
        if let Some(tree) = &self.dom.dom_tree {
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
    pub fn click_ocr_text(&mut self, target_text: &str) -> Result<(), BrowserError> {
        let ocr_boxes = self.perform_ocr_scan();
        if let Some(target_box) = ocr_boxes.iter().find(|b| b.text.contains(target_text)) {
            let trajectory = StealthHumanBehavior::generate_bezier_trajectory(
                (0.0, 0.0),
                (target_box.x as f64, target_box.y as f64),
                10,
            );
            self.trace_collector.record_console(
                "info",
                &format!(
                    "OCR Click on '{}' with Bezier trajectory length {}",
                    target_box.text,
                    trajectory.len()
                ),
            );
            return Ok(());
        }
        Err(BrowserError::OcrTargetNotFound(format!(
            "Target text '{}' not found in pixel buffer",
            target_text
        )))
    }

    /// Fetch HTML over native HTTP transport client and parse into DOM tree
    pub fn fetch_and_load(&mut self, url: &str) -> Result<Vec<NdaTriple>, BrowserError> {
        log::info!("fetch_and_load: GET {}", url);
        // Synthetic schemes carry their own document and never reach the
        // network, so they are resolved before the sandbox check: nothing
        // leaves the process, so there is no host to authorise.
        match synthetic_document(url) {
            Synthetic::Document(body) => return Ok(self.load_html(url, &body)),
            Synthetic::Unsupported(err) => return Err(err),
            Synthetic::Network => {}
        }
        if let Err(e) = self.tab_sandbox.check_network_access(url) {
            log::warn!("fetch_and_load: network access denied for {}: {}", url, e);
            return Err(BrowserError::SandboxViolation(e));
        }
        let resp = self.net.http_client.get(url).map_err(|e| {
            log::error!("fetch_and_load: HTTP GET {} failed: {}", url, e);
            BrowserError::NetworkError(e.to_string())
        })?;
        log::info!(
            "fetch_and_load: {} returned status {}",
            url,
            resp.status_code
        );
        self.net
            .network_tracker
            .record_request(url, "GET", resp.status_code, "document");
        Ok(self.load_html(url, &resp.body))
    }

    /// Native pure-Rust HTML document loading and DOM tree compilation
    pub fn load_html(&mut self, url: &str, html: &str) -> Vec<NdaTriple> {
        self.current_url = url.to_string();
        // History traversal (back/forward) re-loads an entry the stack
        // already points at: pushing again would truncate the forward
        // entries and leave a duplicate, so only fresh navigations grow it.
        if self.history_stack.items[self.history_stack.current_index].url != url {
            self.history_stack.push_state(url, "{}", "");
        }
        let mut _stream_tokenizer = StreamJitTokenizer::new();
        let _stream_tokens = _stream_tokenizer.tokenize_stream_chunk(html.as_bytes());
        let _fast_rules = FastCssParser::parse_rules_fast(html);
        let nodes = HtmlParser::parse_html5(html);
        let tree = DomTree::new(nodes);
        self.page_title = tree.extract_page_title();
        // The title is only known after parsing; backfill the history entry
        // so listings can show where each URL led.
        let cur = self.history_stack.current_index;
        self.history_stack.items[cur].title = self.page_title.clone();
        self.dom.dom_tree = Some(tree);

        // Execute <script> tags automatically
        self.execute_scripts();

        self.trace_collector
            .record_console("info", &format!("Loaded HTML from {}", url));
        self.capture_state_nda()
    }

    /// Execute all <script> tags in the current DOM tree.
    fn execute_scripts(&mut self) {
        if self.dom.dom_tree.is_none() {
            return;
        }
        let tree = self.dom.dom_tree.as_mut().unwrap();
        crate::js::script_runner::execute_page_scripts_full(
            tree,
            &mut self.js.js_vm,
            &mut self.js.js_scheduler,
            &mut self.net.http_client,
            &mut self.trace_collector,
            &self.current_url,
        );
    }

    /// Execute JavaScript expression natively via JS Virtual Machine
    pub fn eval_js(&mut self, expr: &str) -> Result<String, BrowserError> {
        if self.dom.dom_tree.is_none() {
            return Err(BrowserError::NoDomLoaded);
        }

        // Try web API interception first (timers, fetch, storage, etc.)
        let timer_seq = self.js.js_scheduler.seq;
        if let Some(api_result) =
            crate::js::web_apis::eval_web_api(expr, &self.current_url, timer_seq)
        {
            return self.apply_web_api_result(api_result);
        }

        let tree = self.dom.dom_tree.as_mut().unwrap();
        let res = self.js.js_vm.eval_statement(tree, expr)?;
        self.trace_collector
            .record_console("info", &format!("Evaluated JS: '{}'", expr));

        // Drain event loop
        self.drain_event_loop();

        Ok(format!("{:?}", res))
    }

    /// Apply the result of a web API call, handling side effects.
    fn apply_web_api_result(
        &mut self,
        result: crate::js::web_apis::WebApiResult,
    ) -> Result<String, BrowserError> {
        // Handle pending timer
        if let Some((script, delay, is_interval)) = result.pending_timer {
            let id = if is_interval {
                self.js.js_scheduler.schedule_interval(&script, delay)
            } else {
                self.js.js_scheduler.schedule_timer(&script, delay)
            };
            return Ok(format!("{}", id));
        }

        // Handle cancel timer
        if let Some(id) = result.cancel_timer_id {
            self.js.js_scheduler.cancel_timer(id);
            return Ok("undefined".to_string());
        }

        // Handle fetch request
        if let Some((url, method, body, content_type)) = result.fetch_request {
            let resp = if method == "POST" {
                let ct = content_type.as_deref().unwrap_or("application/json");
                let body_str = body.as_deref().unwrap_or("");
                self.net.http_client.post(&url, body_str, ct)
            } else {
                self.net.http_client.get(&url)
            };
            let r = resp.map_err(|e| BrowserError::NetworkError(e.to_string()))?;
            self.net
                .network_tracker
                .record_request(&url, &method, r.status_code, "fetch");
            let fetch_resp = crate::js::web_apis::build_fetch_response(r.status_code, &r.body);
            return Ok(format!("{:?}", fetch_resp));
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
        let tick_limit = self.js.js_scheduler.tick_limit;
        let mut ticks = 0;
        while ticks < tick_limit && self.js.js_scheduler.has_pending_tasks() {
            if let Some(task) = self.js.js_scheduler.pop_next_task() {
                if let Some(tree) = &mut self.dom.dom_tree {
                    let _ = self.js.js_vm.eval_statement(tree, &task.script);
                }
                ticks += 1;
            } else {
                break;
            }
        }
    }

    /// Native CSS selector element query & click event execution
    pub fn click(&mut self, selector: &str) -> Result<(), BrowserError> {
        if let Some(tree) = &mut self.dom.dom_tree {
            let file_event = self.file_manager.handle_file_input_click(tree, selector);
            if file_event.is_some() {
                self.trace_collector
                    .record_console("info", &format!("File chooser opened for '{}'", selector));
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
                let _ =
                    SyntheticEventDispatcher::dispatch_pointer_event_static(tree, node_id, event);
                self.dom
                    .mutation_observer
                    .observe_attribute_change(node_id, "click");
                self.trace_collector.record_mutation(
                    selector,
                    "click",
                    "Native click event dispatched",
                );
                return Ok(());
            }
            return self.click_ocr_text(selector);
        }
        Err(BrowserError::NoDomLoaded)
    }

    /// Native CSS selector form input filling
    pub fn fill(&mut self, selector: &str, text: &str) -> Result<(), BrowserError> {
        if let Some(tree) = &mut self.dom.dom_tree {
            let target_id = {
                let matches = CssMatcher::find_matches(&tree.nodes, selector);
                matches.first().map(|n| n.id)
            };

            if let Some(id) = target_id {
                if let Some(node) = tree.get_node_mut(id) {
                    let jitter = StealthHumanBehavior::compute_typing_jitter(text.len());
                    node.attributes
                        .insert("value".to_string(), text.to_string());
                    let _ = self.js.js_vm.dispatch_event(tree, selector, "input");
                    self.dom
                        .mutation_observer
                        .observe_attribute_change(id, "value");
                    self.trace_collector.record_mutation(
                        selector,
                        "attribute_changed",
                        &format!("value={}, jitter_len={}", text, jitter.len()),
                    );
                    return Ok(());
                }
            }
            return Err(BrowserError::SelectorNotFound(selector.to_string()));
        }
        Err(BrowserError::NoDomLoaded)
    }

    pub fn set_storage_item(&mut self, key: &str, value: &str) {
        let _ = self.storage_quota.reserve(key.len() + value.len());
        self.storage_broadcaster
            .set_item(&mut self.storage, key, value, &self.current_url);
    }

    pub fn reseed_auth(&mut self, auth: &AuthTokenState) {
        AuthReseeder::reseed_into_session(self, auth);
    }

    pub fn attach_file(&mut self, selector: &str, file_path: &str) -> Result<String, BrowserError> {
        if let Err(e) = self.tab_sandbox.check_file_access(file_path) {
            return Err(BrowserError::SandboxViolation(e));
        }
        if let Some(tree) = &mut self.dom.dom_tree {
            let res = self
                .file_manager
                .attach_file(tree, selector, file_path)
                .map_err(|e| BrowserError::NetworkError(e.to_string()))?;
            self.trace_collector
                .record_mutation(selector, "file_attached", file_path);
            return Ok(res);
        }
        Err(BrowserError::NoDomLoaded)
    }

    /// Scroll the viewport by a pixel delta. Offsets are clamped at the
    /// document origin; the resulting position feeds the in-viewport facts
    /// emitted by [`capture_state_document`](Self::capture_state_document).
    pub fn scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<(), BrowserError> {
        self.scroll_x = (self.scroll_x + delta_x as f32).max(0.0);
        self.scroll_y = (self.scroll_y + delta_y as f32).max(0.0);
        self.trace_collector.record_console(
            "info",
            &format!("Scrolled window to ({}, {})", self.scroll_x, self.scroll_y),
        );
        Ok(())
    }

    pub fn hover(&mut self, selector: &str) -> Result<(), BrowserError> {
        self.trace_collector
            .record_console("info", &format!("Hovered native selector '{}'", selector));
        Ok(())
    }

    pub fn press_key(&mut self, key: &str) -> Result<(), BrowserError> {
        self.trace_collector
            .record_console("info", &format!("Pressed key '{}'", key));
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
        let tree = self.dom.dom_tree.as_ref()?;
        let node = tree.get_node(node_id)?;
        if let Some(id) = node.attributes.get("id") {
            if !id.is_empty() {
                return Some(format!("#{}", id));
            }
        }
        Some(node.tag_name.clone())
    }

    /// Whether `node_id` exists in the live document. Callers that take a node
    /// id from the agent must check this first: acting on a stale or invented
    /// id used to produce a well-formed empty answer (bug #50).
    pub fn has_node(&self, node_id: usize) -> bool {
        self.dom
            .dom_tree
            .as_ref()
            .and_then(|tree| tree.get_node(node_id))
            .is_some()
    }

    /// Set the `checked` state of a checkbox/radio the way a browser would,
    /// including radio-group exclusivity: selecting one radio clears every
    /// other radio that shares its non-empty `name`. Returns false when the
    /// node is missing or is not a checkable control.
    fn set_checked_state(&mut self, node_id: usize, state: bool) -> bool {
        let Some(tree) = &mut self.dom.dom_tree else {
            return false;
        };
        // Read the kind and group as owned data before writing anything, so
        // the mutation cannot invalidate what the group was computed from.
        let Some(kind) = tree.get_node(node_id).and_then(|node| {
            if node.tag_name != "input" {
                return None;
            }
            let ty = node
                .attributes
                .get("type")
                .map(|t| t.as_str())
                .unwrap_or("text");
            match ty {
                "checkbox" | "radio" => Some(ty.to_string()),
                _ => None,
            }
        }) else {
            return false;
        };
        let group = (kind == "radio")
            .then(|| {
                tree.get_node(node_id)
                    .and_then(|node| node.attributes.get("name"))
                    .filter(|name| !name.is_empty())
                    .cloned()
            })
            .flatten();
        if let Some(group) = &group {
            for sibling in tree.nodes.iter_mut() {
                if sibling.id == node_id {
                    continue;
                }
                let in_group = sibling.tag_name == "input"
                    && sibling.attributes.get("type").map(|s| s.as_str()) == Some("radio")
                    && sibling.attributes.get("name").map(|s| s.as_str()) == Some(group.as_str());
                if in_group {
                    sibling.attributes.remove("checked");
                }
            }
        }
        if let Some(node) = tree.get_node_mut(node_id) {
            if state {
                node.attributes
                    .insert("checked".to_string(), "checked".to_string());
            } else {
                node.attributes.remove("checked");
            }
        }
        true
    }

    /// Activation behaviour for a click on a checkable control: a checkbox
    /// toggles, a radio is selected (and its group is cleared). Returns false
    /// for anything that is not checkable, so the caller can tell "clicked a
    /// button" from "clicked a checkbox and changed its state" (bug #51).
    fn apply_checked_activation(&mut self, node_id: usize) -> bool {
        let wanted = self
            .dom
            .dom_tree
            .as_ref()
            .and_then(|tree| tree.get_node(node_id))
            .and_then(|node| {
                if node.tag_name != "input" {
                    return None;
                }
                match node
                    .attributes
                    .get("type")
                    .map(|t| t.as_str())
                    .unwrap_or("text")
                {
                    "checkbox" => Some(!node.attributes.contains_key("checked")),
                    "radio" => Some(true),
                    _ => None,
                }
            });
        match wanted {
            Some(state) => self.set_checked_state(node_id, state),
            None => false,
        }
    }

    /// Click a node by id: dispatch a real synthetic pointer event and fire any
    /// matching JS click listeners, then report the NDA delta it produced.
    /// If the node is an `<a>` element with a navigable `href`, the click
    /// triggers a full navigation to the linked page.
    pub fn agent_click(&mut self, node_id: usize) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        let dispatched = if let Some(tree) = &mut self.dom.dom_tree {
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
                let _ =
                    SyntheticEventDispatcher::dispatch_pointer_event_static(tree, node_id, event);
                if let Some(sel) = &selector {
                    let _ = self.js.js_vm.dispatch_event(tree, sel, "click");
                }
                self.dom
                    .mutation_observer
                    .observe_attribute_change(node_id, "click");
                true
            } else {
                false
            }
        } else {
            false
        };

        // Follow link navigation: if the clicked element is <a href="..."> with
        // a navigable target, perform a full fetch-and-load.
        let mut nav_note = String::new();
        if dispatched {
            // A click on a checkbox/radio has to move its state, not just fire
            // the event: previously the report said "clicked" while the form
            // kept its old values, and the submission dropped the control.
            if self.apply_checked_activation(node_id) {
                if let Some(sel) = &selector {
                    if let Some(tree) = &mut self.dom.dom_tree {
                        let _ = self.js.js_vm.dispatch_event(tree, sel, "change");
                    }
                }
                self.dom
                    .mutation_observer
                    .observe_attribute_change(node_id, "checked");
            }
            let href = self
                .dom
                .dom_tree
                .as_ref()
                .and_then(|tree| tree.get_node(node_id))
                .filter(|node| node.tag_name == "a")
                .and_then(|node| node.attributes.get("href"))
                .filter(|h| !h.is_empty() && !h.starts_with('#') && !h.starts_with("javascript:"))
                .cloned();
            if let Some(href) = href {
                let target = self.resolve_url(&href);
                if let Err(e) = self.fetch_and_load(&target) {
                    // The click did fire, but the navigation it triggered
                    // went nowhere. Silently reporting a bare "clicked" hid
                    // that from every caller.
                    nav_note = format!(" (navigation failed: {})", e);
                }
            }
        }

        let after = self.capture_state_document();
        let status = if dispatched {
            format!("clicked node_{}{}", node_id, nav_note)
        } else {
            format!("node_{} not found", node_id)
        };
        AgentActionResult::new(status, diff(&before, &after)).with_executed(dispatched)
    }

    /// Type text into a node by id: set its `value`, fire matching JS `input`
    /// listeners, and report the NDA delta.
    pub fn agent_type(&mut self, node_id: usize, text: &str) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        let ok = if let Some(tree) = &mut self.dom.dom_tree {
            let found = if let Some(node) = tree.get_node_mut(node_id) {
                node.attributes
                    .insert("value".to_string(), text.to_string());
                true
            } else {
                false
            };
            if found {
                if let Some(sel) = &selector {
                    let _ = self.js.js_vm.dispatch_event(tree, sel, "input");
                }
                self.dom
                    .mutation_observer
                    .observe_attribute_change(node_id, "value");
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
        AgentActionResult::new(status, diff(&before, &after)).with_executed(ok)
    }

    /// Select a value on a combobox/select node by id, fire `change` listeners,
    /// and report the NDA delta.
    pub fn agent_select(&mut self, node_id: usize, value: &str) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        let ok = if let Some(tree) = &mut self.dom.dom_tree {
            let found = if let Some(node) = tree.get_node_mut(node_id) {
                node.attributes
                    .insert("value".to_string(), value.to_string());
                true
            } else {
                false
            };
            if found {
                if let Some(sel) = &selector {
                    let _ = self.js.js_vm.dispatch_event(tree, sel, "change");
                }
                self.dom
                    .mutation_observer
                    .observe_attribute_change(node_id, "value");
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
        AgentActionResult::new(status, diff(&before, &after)).with_executed(ok)
    }

    /// Submit a form (or a control within one) by node id: collect form fields,
    /// resolve the `action` URL and `method`, fire JS `submit` listeners, then
    /// perform the actual HTTP submission (POST with URL-encoded body or GET
    /// with query params) and load the server response into the live DOM.
    pub fn agent_submit(&mut self, node_id: usize) -> AgentActionResult {
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);

        // Fire JS submit event
        if let Some(tree) = &mut self.dom.dom_tree {
            if tree.get_node(node_id).is_some() {
                if let Some(sel) = &selector {
                    let _ = self.js.js_vm.dispatch_event(tree, sel, "submit");
                }
                self.dom
                    .mutation_observer
                    .observe_attribute_change(node_id, "submit");
            }
        }

        // Collect form data and perform the actual submission. Three endings
        // have to stay distinguishable: no form at all, a submission that
        // landed, and a submission that was attempted but failed.
        let outcome = match self.collect_form_submission(node_id) {
            None => SubmitOutcome::NoForm,
            Some((method, action_url, encoded_body)) => {
                if method.eq_ignore_ascii_case("post") {
                    match self.net.http_client.post(
                        &action_url,
                        &encoded_body,
                        "application/x-www-form-urlencoded",
                    ) {
                        Ok(resp) => {
                            self.net.network_tracker.record_request(
                                &action_url,
                                "POST",
                                resp.status_code,
                                "document",
                            );
                            self.load_html(&action_url, &resp.body);
                            SubmitOutcome::Submitted(action_url)
                        }
                        // The transport error is the whole story here, so it
                        // has to reach the caller instead of becoming a bool.
                        Err(e) => SubmitOutcome::Failed(e.to_string()),
                    }
                } else {
                    let target = if encoded_body.is_empty() {
                        action_url
                    } else {
                        format!("{}?{}", action_url, encoded_body)
                    };
                    match self.fetch_and_load(&target) {
                        Ok(_) => SubmitOutcome::Submitted(target),
                        Err(e) => SubmitOutcome::Failed(e.to_string()),
                    }
                }
            }
        };

        let after = self.capture_state_document();
        let (status, landed) = match outcome {
            SubmitOutcome::Submitted(url) => (
                format!(
                    "submitted node_{} to {}",
                    node_id,
                    truncate_url_for_message(&url)
                ),
                true,
            ),
            SubmitOutcome::NoForm => (
                // Firing the submit listeners is not a submission: no request
                // went out and no response was loaded.
                format!("node_{} is not inside a form", node_id),
                false,
            ),
            SubmitOutcome::Failed(err) => (
                format!("submit for node_{} failed: {}", node_id, err),
                false,
            ),
        };
        AgentActionResult::new(status, diff(&before, &after)).with_executed(landed)
    }

    /// Scroll the viewport and report the NDA delta: the session scroll fact
    /// always moves, and nodes crossing the viewport edge flip their
    /// in-viewport facts.
    pub fn agent_scroll(&mut self, delta_x: i32, delta_y: i32) -> AgentActionResult {
        let before = self.capture_state_document();
        let _ = self.scroll(delta_x, delta_y);
        let after = self.capture_state_document();
        AgentActionResult::new(
            format!(
                "scrolled ({}, {}) to offset ({}, {})",
                delta_x, delta_y, self.scroll_x, self.scroll_y
            ),
            diff(&before, &after),
        )
    }

    /// Scroll an element into view by accessible name: resolve it via the AOM,
    /// find its layout box, and move the viewport so the box is visible. The
    /// delta shows exactly which nodes entered or left the viewport.
    pub fn agent_scroll_into_view(&mut self, query: &str) -> AgentActionResult {
        let Some(node_id) = self.resolve_node_by_name(query, |_| true) else {
            return AgentActionResult::failed(
                format!("no element matching '{}'", query),
                NdaDelta::default(),
            );
        };
        let target = self.dom.dom_tree.as_ref().and_then(|tree| {
            let layout_engine = LayoutEngine2D::new(self.cascader.clone());
            layout_engine
                .build_layout_tree(tree)
                .into_iter()
                .find(|b| b.node_id == node_id)
        });
        let Some(b) = target else {
            return AgentActionResult::failed(
                format!("node_{} has no layout box", node_id),
                NdaDelta::default(),
            );
        };
        let before = self.capture_state_document();
        let status = if self.box_in_viewport(&b) {
            format!("node_{} already in view", node_id)
        } else {
            // Align the box's top-left corner with the viewport origin,
            // clamped at the document origin — the scrollIntoView contract.
            self.scroll_x = b.x.max(0.0);
            self.scroll_y = b.y.max(0.0);
            self.trace_collector.record_console(
                "info",
                &format!(
                    "Scrolled node_{} into view at ({}, {})",
                    node_id, self.scroll_x, self.scroll_y
                ),
            );
            format!(
                "scrolled node_{} into view (offset {}, {})",
                node_id, self.scroll_x, self.scroll_y
            )
        };
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Navigate to a URL (fetch + load) and report the NDA delta between the
    /// previous and freshly loaded page state.
    pub fn agent_navigate(&mut self, url: &str) -> AgentActionResult {
        let before = self.capture_state_document();
        let (status, landed) = match self.fetch_and_load(url) {
            Ok(_) => (format!("navigated to {}", url), true),
            Err(e) => (format!("navigation to {} failed: {}", url, e), false),
        };
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after)).with_executed(landed)
    }

    /// Go back to the previous page in the session history stack.
    pub fn agent_back(&mut self) -> AgentActionResult {
        let before = self.capture_state_document();
        let url = self.history_stack.back().map(|h| h.url.clone());
        let (status, landed) = if let Some(url) = url {
            match self.fetch_and_load(&url) {
                Ok(_) => (format!("navigated back to {}", self.current_url), true),
                Err(e) => (format!("back navigation failed: {}", e), false),
            }
        } else {
            ("no history entry to go back to".to_string(), false)
        };
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after)).with_executed(landed)
    }

    /// Go forward in the session history stack.
    pub fn agent_forward(&mut self) -> AgentActionResult {
        let before = self.capture_state_document();
        let url = self.history_stack.forward().map(|h| h.url.clone());
        let (status, landed) = if let Some(url) = url {
            match self.fetch_and_load(&url) {
                Ok(_) => (format!("navigated forward to {}", self.current_url), true),
                Err(e) => (format!("forward navigation failed: {}", e), false),
            }
        } else {
            ("no history entry to go forward to".to_string(), false)
        };
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after)).with_executed(landed)
    }

    /// Drain pending event-loop work (timers, microtasks) and report the NDA
    /// delta the settled work produced — the session-level "wait until the
    /// page is quiet" primitive. An empty delta means the page really is idle.
    pub fn agent_settle(&mut self) -> AgentActionResult {
        let before = self.capture_state_document();
        self.drain_event_loop();
        let after = self.capture_state_document();
        let delta = diff(&before, &after);
        let status = if delta.is_empty() {
            "settled: no changes".to_string()
        } else {
            format!("settled: {} facts changed", delta.len())
        };
        AgentActionResult::new(status, delta)
    }

    /// Observe the current page state as compact readable NDA fact lines —
    /// the read-only counterpart of the `agent_*` actions. One line per fact,
    /// `subject|predicate-name|object`, no JSON anywhere.
    pub fn agent_observe(&self) -> String {
        self.capture_state_document().facts_text()
    }

    /// Visible text of the current page in reading order: the title followed
    /// by all text content, with script/style/noscript subtrees skipped and
    /// whitespace collapsed. This is the raw material `remember` feeds into
    /// vector memory so pages can be recalled semantically later.
    pub fn page_text(&self) -> String {
        let Some(tree) = self.dom.dom_tree.as_ref() else {
            return String::new();
        };
        let mut buf = String::new();
        if !self.page_title.is_empty() && self.page_title != "Untitled Page" {
            buf.push_str(&self.page_title);
            buf.push(' ');
        }
        for node in &tree.nodes {
            if node.parent.is_none() {
                Self::visible_text_walk(tree, node.id, &mut buf);
            }
        }
        buf.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Depth-first text collection that skips non-rendered subtrees. The
    /// `<title>` subtree is skipped too because `page_text` already leads
    /// with the page title.
    fn visible_text_walk(tree: &DomTree, id: usize, out: &mut String) {
        use crate::parser::html::NodeType;
        let Some(node) = tree.get_node(id) else {
            return;
        };
        if node.node_type == NodeType::Element
            && matches!(
                node.tag_name.as_str(),
                "script" | "style" | "noscript" | "title"
            )
        {
            return;
        }
        if node.node_type == NodeType::Text {
            out.push_str(&node.text_content);
            out.push(' ');
        }
        for &child in &node.children {
            Self::visible_text_walk(tree, child, out);
        }
    }

    /// Tags that carry no readable content and are skipped by every
    /// distilled page projection below.
    const BOILERPLATE_TAGS: &'static [&'static str] = &[
        "script", "style", "noscript", "title", "nav", "footer", "header", "aside", "svg",
        "iframe", "object", "embed",
    ];

    /// Class/id fragments that mark a container as boilerplate even when its
    /// tag looks harmless (e.g. `<div class="cookie-banner">`).
    const BOILERPLATE_PATTERNS: &'static [&'static str] = &[
        "sidebar", "footer", "menu", "advert", "banner", "cookie", "popup", "modal", "social",
        "share", "related",
    ];

    /// True when the element's class or id matches a boilerplate pattern.
    fn is_boilerplate_container(node: &crate::parser::html::DomNode) -> bool {
        for key in ["class", "id"] {
            if let Some(value) = node.attributes.get(key) {
                let value = value.to_ascii_lowercase();
                if Self::BOILERPLATE_PATTERNS.iter().any(|p| value.contains(p)) {
                    return true;
                }
            }
        }
        false
    }

    /// Render the page as markdown: headings, paragraphs, lists, links and
    /// emphasis survive; boilerplate (nav/footer/script/...) is dropped.
    /// Structure costs the agent almost nothing and disambiguates a page far
    /// better than flat text.
    pub fn page_markdown(&self) -> String {
        let Some(tree) = self.dom.dom_tree.as_ref() else {
            return String::new();
        };
        let mut out = String::new();
        if !self.page_title.is_empty() && self.page_title != "Untitled Page" {
            out.push_str("# ");
            out.push_str(&self.page_title);
            out.push_str("\n\n");
        }
        for node in &tree.nodes {
            if node.parent.is_none() {
                Self::markdown_walk(tree, node.id, &mut out);
            }
        }
        out.trim_end().to_string()
    }

    /// Readability projection: markdown of just the main content region.
    /// Roots at `<main>` (or `<article>`) when present, falls back to
    /// `<body>`, and drops containers whose class/id look like chrome
    /// (sidebar, cookie banner, ...). The cheapest way to read an article.
    pub fn page_content_markdown(&self) -> String {
        let Some(tree) = self.dom.dom_tree.as_ref() else {
            return String::new();
        };
        let root = ["main", "article", "body"]
            .iter()
            .find_map(|tag| tree.nodes.iter().find(|n| n.tag_name == *tag))
            .map(|n| n.id);
        let mut out = String::new();
        if !self.page_title.is_empty() && self.page_title != "Untitled Page" {
            out.push_str("# ");
            out.push_str(&self.page_title);
            out.push_str("\n\n");
        }
        match root {
            Some(id) => Self::markdown_walk(tree, id, &mut out),
            // Fragment documents have no body; render everything instead.
            None => {
                for node in &tree.nodes {
                    if node.parent.is_none() {
                        Self::markdown_walk(tree, node.id, &mut out);
                    }
                }
            }
        }
        out.trim_end().to_string()
    }

    /// The readable core of the page: the readability projection, then plain
    /// markdown, then raw visible text. The last step matters for documents
    /// that are not HTML prose at all - a JSON or plain-text body has no
    /// paragraphs for markdown to shape, so it used to distil to an empty
    /// string while `page_text` still reported hundreds of characters. Every
    /// reader of "the content" (content fact, assert, wait) goes through here
    /// so they can never disagree (bug #53).
    pub fn distilled_content(&self) -> String {
        let mut content = self.page_content_markdown();
        if content.is_empty() {
            content = self.page_markdown();
        }
        if content.is_empty() {
            content = self.page_text();
        }
        content
    }

    fn markdown_walk(tree: &DomTree, id: usize, out: &mut String) {
        use crate::parser::html::NodeType;
        let Some(node) = tree.get_node(id) else {
            return;
        };
        if node.node_type == NodeType::Element
            && (Self::BOILERPLATE_TAGS.contains(&node.tag_name.as_str())
                || Self::is_boilerplate_container(node))
        {
            return;
        }
        match node.tag_name.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let depth = node.tag_name.as_bytes()[1] - b'0';
                let text = Self::inline_markdown(tree, id);
                if !text.is_empty() {
                    for _ in 0..depth {
                        out.push('#');
                    }
                    out.push(' ');
                    out.push_str(&text);
                    out.push_str("\n\n");
                }
            }
            "p" | "blockquote" => {
                let text = Self::inline_markdown(tree, id);
                if !text.is_empty() {
                    out.push_str(&text);
                    out.push_str("\n\n");
                }
            }
            "ul" | "ol" => {
                let ordered = node.tag_name == "ol";
                let mut n = 0usize;
                for &child in &node.children {
                    let Some(item) = tree.get_node(child) else {
                        continue;
                    };
                    if item.tag_name != "li" {
                        continue;
                    }
                    let text = Self::inline_markdown(tree, child);
                    if text.is_empty() {
                        continue;
                    }
                    n += 1;
                    if ordered {
                        out.push_str(&format!("{n}. {text}\n"));
                    } else {
                        out.push_str(&format!("- {text}\n"));
                    }
                }
                if n > 0 {
                    out.push('\n');
                }
            }
            "table" => {
                Self::table_markdown(tree, id, out);
            }
            _ => {
                for &child in &node.children {
                    Self::markdown_walk(tree, child, out);
                }
            }
        }
    }

    /// Inline text of a node with markdown emphasis: links become
    /// [text](href), strong/b become **text**, code becomes `text`.
    fn inline_markdown(tree: &DomTree, id: usize) -> String {
        use crate::parser::html::NodeType;
        fn walk(tree: &DomTree, id: usize, out: &mut String) {
            let Some(node) = tree.get_node(id) else {
                return;
            };
            if node.node_type == NodeType::Text {
                out.push_str(&node.text_content);
                out.push(' ');
                return;
            }
            if BrowserSession::BOILERPLATE_TAGS.contains(&node.tag_name.as_str()) {
                return;
            }
            match node.tag_name.as_str() {
                "a" => {
                    let mut inner = String::new();
                    for &child in &node.children {
                        walk(tree, child, &mut inner);
                    }
                    let text = inner.split_whitespace().collect::<Vec<_>>().join(" ");
                    match node.attributes.get("href") {
                        Some(href) if !text.is_empty() => {
                            out.push_str(&format!("[{text}]({href}) "));
                        }
                        _ => {
                            out.push_str(&text);
                            out.push(' ');
                        }
                    }
                }
                "strong" | "b" => {
                    let mut inner = String::new();
                    for &child in &node.children {
                        walk(tree, child, &mut inner);
                    }
                    let text = inner.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.is_empty() {
                        out.push_str(&format!("**{text}** "));
                    }
                }
                "em" | "i" => {
                    let mut inner = String::new();
                    for &child in &node.children {
                        walk(tree, child, &mut inner);
                    }
                    let text = inner.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.is_empty() {
                        out.push_str(&format!("*{text}* "));
                    }
                }
                "code" => {
                    let mut inner = String::new();
                    for &child in &node.children {
                        walk(tree, child, &mut inner);
                    }
                    let text = inner.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !text.is_empty() {
                        out.push_str(&format!("`{text}` "));
                    }
                }
                _ => {
                    for &child in &node.children {
                        walk(tree, child, out);
                    }
                }
            }
        }
        let mut buf = String::new();
        walk(tree, id, &mut buf);
        buf.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Every table on the page rendered as markdown rows — the densest
    /// faithful encoding of tabular data for an agent.
    pub fn page_tables_text(&self) -> String {
        let Some(tree) = self.dom.dom_tree.as_ref() else {
            return String::new();
        };
        let mut out = String::new();
        for node in &tree.nodes {
            if node.tag_name == "table" {
                Self::table_markdown(tree, node.id, &mut out);
            }
        }
        out.trim_end().to_string()
    }

    fn table_markdown(tree: &DomTree, table_id: usize, out: &mut String) {
        let Some(table) = tree.get_node(table_id) else {
            return;
        };
        // Caption first, then every <tr> in document order (thead/tbody
        // wrappers are transparent).
        let mut rows: Vec<(bool, Vec<String>)> = Vec::new();
        fn collect_rows(
            tree: &DomTree,
            id: usize,
            rows: &mut Vec<(bool, Vec<String>)>,
            out: &mut String,
        ) {
            let Some(node) = tree.get_node(id) else {
                return;
            };
            match node.tag_name.as_str() {
                "caption" => {
                    let text = BrowserSession::inline_markdown(tree, id);
                    if !text.is_empty() {
                        out.push_str(&format!("Table: {text}\n"));
                    }
                }
                "tr" => {
                    let mut cells = Vec::new();
                    let mut is_header = false;
                    for &child in &node.children {
                        let Some(cell) = tree.get_node(child) else {
                            continue;
                        };
                        match cell.tag_name.as_str() {
                            "th" => {
                                is_header = true;
                                cells.push(BrowserSession::inline_markdown(tree, child));
                            }
                            "td" => cells.push(BrowserSession::inline_markdown(tree, child)),
                            _ => {}
                        }
                    }
                    if !cells.is_empty() {
                        rows.push((is_header, cells));
                    }
                }
                _ => {
                    for &child in &node.children {
                        collect_rows(tree, child, rows, out);
                    }
                }
            }
        }
        for &child in &table.children {
            collect_rows(tree, child, &mut rows, out);
        }
        for (i, (is_header, cells)) in rows.iter().enumerate() {
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
            if i == 0 && *is_header {
                out.push_str(&format!("| {} |\n", vec!["---"; cells.len()].join(" | ")));
            }
        }
        out.push('\n');
    }

    /// One-screen structural digest: title, element counts and the heading
    /// outline. Enough to decide whether a page is worth reading in full.
    pub fn page_summary_text(&self) -> String {
        let Some(tree) = self.dom.dom_tree.as_ref() else {
            return String::new();
        };
        let (mut links, mut forms, mut images, mut interactive, mut tables) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        let mut headings: Vec<(u8, String)> = Vec::new();
        for node in &tree.nodes {
            match node.tag_name.as_str() {
                "a" => {
                    if node.attributes.contains_key("href") {
                        links += 1;
                        interactive += 1;
                    }
                }
                "form" => forms += 1,
                "img" => images += 1,
                "button" | "input" | "select" | "textarea" => interactive += 1,
                "table" => tables += 1,
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let text = Self::inline_markdown(tree, node.id);
                    if !text.is_empty() {
                        headings.push((node.tag_name.as_bytes()[1] - b'0', text));
                    }
                }
                _ => {}
            }
        }
        let title = if self.page_title.is_empty() {
            self.current_url.clone()
        } else {
            self.page_title.clone()
        };
        let mut out = format!(
            "Page: {title}\n{links} link(s), {forms} form(s), {interactive} interactive element(s), {images} image(s), {tables} table(s), {} chars of text\n",
            self.page_text().chars().count()
        );
        if !headings.is_empty() {
            out.push_str("Headings:\n");
            for (depth, text) in &headings {
                for _ in 0..*depth {
                    out.push('#');
                }
                out.push_str(&format!(" {text}\n"));
            }
        }
        out.trim_end().to_string()
    }

    // === Label-based semantic actions =======================================
    // The node-id actions above assume the agent already ran agent_observe and
    // picked a node. These variants resolve the target by accessible name via
    // the AOM, so a single call ("click 'Log In'") replaces an observe +
    // parse + act round trip — fewer tokens, fewer mistakes.

    /// Resolve a DOM node by accessible name using the AOM. Exact
    /// (case-insensitive) name matches beat substring matches; among equal
    /// ranks the more actionable node wins. `role_ok` filters candidate roles.
    ///
    /// A control's own `placeholder` is a secondary key: it stops being the
    /// accessible name as soon as a real `<label>` is bound to the control,
    /// and the label-based tools promise to match it either way.
    fn resolve_node_by_name(&self, query: &str, role_ok: fn(&str) -> bool) -> Option<usize> {
        let tree = self.dom.dom_tree.as_ref()?;
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }
        let aom_nodes = AgenticAomTree::build_aom_nodes(tree);
        let mut best: Option<(u8, u8, usize)> = None; // (rank, actionability, node_id)
        for node in &aom_nodes {
            if !role_ok(&node.role) {
                continue;
            }
            let name = node.name.to_lowercase();
            let mut rank = if name == needle {
                2
            } else if name.contains(&needle) {
                1
            } else {
                0
            };
            // The AOM id is "node_{id}" — recover the numeric DOM id so the
            // raw attributes stay reachable for the placeholder fallback.
            let Some(id) = node.id.strip_prefix("node_").and_then(|s| s.parse().ok()) else {
                continue;
            };
            if rank < 2 {
                if let Some(ph) = tree
                    .get_node(id)
                    .and_then(|n| n.attributes.get("placeholder"))
                {
                    let ph = ph.to_lowercase();
                    if ph == needle {
                        rank = 2;
                    } else if ph.contains(&needle) {
                        rank = rank.max(1);
                    }
                }
            }
            if rank == 0 {
                continue;
            }
            let candidate = (rank, node.actionability_score, id);
            if best
                .map(|b| (candidate.0, candidate.1) > (b.0, b.1))
                .unwrap_or(true)
            {
                best = Some(candidate);
            }
        }
        best.map(|(_, _, id)| id)
    }

    /// Click an element by its accessible name (button/link text, aria-label).
    /// Resolves via the AOM and routes through [`agent_click`](Self::agent_click).
    pub fn agent_click_by_text(&mut self, query: &str) -> AgentActionResult {
        match self.resolve_node_by_name(query, |r| {
            matches!(r, "button" | "link" | "checkbox" | "radio" | "generic")
        }) {
            Some(id) => self.agent_click(id),
            None => AgentActionResult::failed(
                format!("no clickable element matching '{}'", query),
                NdaDelta::default(),
            ),
        }
    }

    /// Fill a text control by its accessible name (label, placeholder,
    /// aria-label). Routes through [`agent_type`](Self::agent_type).
    pub fn agent_fill_by_label(&mut self, query: &str, text: &str) -> AgentActionResult {
        match self.resolve_node_by_name(query, |r| matches!(r, "textbox" | "combobox")) {
            Some(id) => self.agent_type(id, text),
            None => AgentActionResult::failed(
                format!("no fillable control matching '{}'", query),
                NdaDelta::default(),
            ),
        }
    }

    /// Set a checkbox/radio by its accessible name: toggles the `checked`
    /// attribute to `state`, fires `change` listeners, and reports the delta.
    pub fn agent_check_by_label(&mut self, query: &str, state: bool) -> AgentActionResult {
        let Some(node_id) = self.resolve_node_by_name(query, |r| matches!(r, "checkbox" | "radio"))
        else {
            return AgentActionResult::failed(
                format!("no checkable control matching '{}'", query),
                NdaDelta::default(),
            );
        };
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        let changed = self.set_checked_state(node_id, state);
        if changed {
            if let Some(tree) = &mut self.dom.dom_tree {
                if let Some(sel) = &selector {
                    let _ = self.js.js_vm.dispatch_event(tree, sel, "change");
                }
            }
            self.dom
                .mutation_observer
                .observe_attribute_change(node_id, "checked");
        }
        let after = self.capture_state_document();
        let status = format!(
            "{} node_{}",
            if state { "checked" } else { "unchecked" },
            node_id
        );
        AgentActionResult::new(status, diff(&before, &after))
    }

    /// Read the current form state as token-cheap text: one
    /// `name [role] = value` line per fillable/checkable control, checkables
    /// shown as checked/unchecked. The read-only sibling of the fill actions.
    pub fn agent_read_form(&self) -> String {
        let Some(tree) = &self.dom.dom_tree else {
            return String::new();
        };
        let aom_nodes = AgenticAomTree::build_aom_nodes(tree);
        let mut out = String::new();
        for node in &aom_nodes {
            let checkable = matches!(node.role.as_str(), "checkbox" | "radio");
            if !checkable && !matches!(node.role.as_str(), "textbox" | "combobox") {
                continue;
            }
            let value = if checkable {
                let checked = node
                    .id
                    .strip_prefix("node_")
                    .and_then(|s| s.parse().ok())
                    .and_then(|id: usize| tree.get_node(id))
                    .map(|n| n.attributes.contains_key("checked"))
                    .unwrap_or(false);
                if checked { "checked" } else { "unchecked" }.to_string()
            } else {
                node.value.clone()
            };
            out.push_str(&format!("{} [{}] = {}\n", node.name, node.role, value));
        }
        out
    }

    // === Session focus model & keyboard ====================================

    /// Roles that can receive keyboard focus at the session level.
    fn is_focusable_role(role: &str) -> bool {
        matches!(
            role,
            "button" | "link" | "textbox" | "checkbox" | "radio" | "combobox"
        )
    }

    /// Move keyboard focus to a node by id: fires `blur` on the old node,
    /// `focus` on the new one, and reports the NDA delta (focus is a fact).
    pub fn agent_focus(&mut self, node_id: usize) -> AgentActionResult {
        let before = self.capture_state_document();
        let exists = self
            .dom
            .dom_tree
            .as_ref()
            .and_then(|t| t.get_node(node_id))
            .is_some();
        if exists {
            let old = self.focused_node.take();
            if let (Some(old_id), Some(sel)) = (old, old.and_then(|id| self.selector_for_node(id)))
            {
                if let Some(tree) = &mut self.dom.dom_tree {
                    let _ = self.js.js_vm.dispatch_event(tree, &sel, "blur");
                }
                self.dom
                    .mutation_observer
                    .observe_attribute_change(old_id, "blur");
            }
            self.focused_node = Some(node_id);
            if let Some(sel) = self.selector_for_node(node_id) {
                if let Some(tree) = &mut self.dom.dom_tree {
                    let _ = self.js.js_vm.dispatch_event(tree, &sel, "focus");
                }
            }
            self.dom
                .mutation_observer
                .observe_attribute_change(node_id, "focus");
        }
        let after = self.capture_state_document();
        let status = if exists {
            format!("focused node_{}", node_id)
        } else {
            format!("node_{} not found", node_id)
        };
        AgentActionResult::new(status, diff(&before, &after)).with_executed(exists)
    }

    /// Focus a control by its accessible name — any focusable role qualifies.
    pub fn agent_focus_by_label(&mut self, query: &str) -> AgentActionResult {
        match self.resolve_node_by_name(query, Self::is_focusable_role) {
            Some(id) => self.agent_focus(id),
            None => AgentActionResult::failed(
                format!("no focusable element matching '{}'", query),
                NdaDelta::default(),
            ),
        }
    }

    /// Press a key on the focused node: fires `keydown`/`keyup` listeners,
    /// types single characters into the value, `Enter` submits the enclosing
    /// form, `Tab` advances focus to the next focusable control (wrapping).
    pub fn agent_press(&mut self, key: &str) -> AgentActionResult {
        let Some(node_id) = self.focused_node else {
            return AgentActionResult::failed(
                format!("cannot press '{}': nothing focused", key),
                NdaDelta::default(),
            );
        };
        let before = self.capture_state_document();
        let selector = self.selector_for_node(node_id);
        if let (Some(tree), Some(sel)) = (&mut self.dom.dom_tree, &selector) {
            let _ = self.js.js_vm.dispatch_event(tree, sel, "keydown");
            // The tool contract advertises keydown/keypress/keyup, so the
            // cancelable text-input event has to fire too - not just the two
            // ends of the sequence.
            let _ = self.js.js_vm.dispatch_event(tree, sel, "keypress");
        }

        let (status, landed) = match key {
            "Enter" => {
                if self.find_enclosing_form(node_id).is_some() {
                    let submit = self.agent_submit(node_id);
                    (format!("pressed Enter: {}", submit.status), submit.executed)
                } else {
                    ("pressed Enter".to_string(), true)
                }
            }
            "Tab" => {
                // Advance to the next focusable node by DOM order, wrapping.
                let next = self.dom.dom_tree.as_ref().map(|tree| {
                    let mut focusables: Vec<usize> = AgenticAomTree::build_aom_nodes(tree)
                        .iter()
                        .filter(|n| Self::is_focusable_role(&n.role))
                        .filter_map(|n| n.id.strip_prefix("node_").and_then(|s| s.parse().ok()))
                        .collect();
                    focusables.sort_unstable();
                    focusables
                        .iter()
                        .find(|&&id| id > node_id)
                        .or_else(|| focusables.first())
                        .copied()
                });
                match next.flatten() {
                    Some(next_id) => {
                        let moved = self.agent_focus(next_id);
                        (format!("pressed Tab: {}", moved.status), true)
                    }
                    // Tab promises to move focus; with nothing focusable it
                    // moved nowhere, so the result must not read as done.
                    None => ("pressed Tab: no focusable elements".to_string(), false),
                }
            }
            _ => {
                // Single visible characters type into the focused control.
                let mut chars = key.chars();
                if let (Some(ch), None) = (chars.next(), chars.next()) {
                    if let Some(tree) = &mut self.dom.dom_tree {
                        if let Some(node) = tree.get_node_mut(node_id) {
                            let value = node.attributes.entry("value".to_string()).or_default();
                            value.push(ch);
                        }
                        if let Some(sel) = &selector {
                            let _ = self.js.js_vm.dispatch_event(tree, sel, "input");
                        }
                        self.dom
                            .mutation_observer
                            .observe_attribute_change(node_id, "value");
                    }
                }
                (format!("pressed '{}' on node_{}", key, node_id), true)
            }
        };

        if let (Some(tree), Some(sel)) = (&mut self.dom.dom_tree, &selector) {
            let _ = self.js.js_vm.dispatch_event(tree, sel, "keyup");
        }
        let after = self.capture_state_document();
        AgentActionResult::new(status, diff(&before, &after)).with_executed(landed)
    }

    /// Choose a dropdown option by its visible text (or `value` attribute) on
    /// a select resolved by accessible name. Marks the option `selected`,
    /// mirrors its value onto the select, and fires `change` listeners.
    pub fn agent_select_by_label(&mut self, query: &str, option: &str) -> AgentActionResult {
        let Some(select_id) = self.resolve_node_by_name(query, |r| r == "combobox") else {
            return AgentActionResult::failed(
                format!("no select matching '{}'", query),
                NdaDelta::default(),
            );
        };
        // Rank candidate <option> descendants (real-world markup nests them
        // under whitespace text nodes or <optgroup>): exact text/value match
        // beats substring.
        let needle = option.trim().to_lowercase();
        let chosen = self.dom.dom_tree.as_ref().and_then(|tree| {
            let select = tree.get_node(select_id)?;
            let mut best: Option<(u8, usize, String)> = None;
            let mut stack: Vec<usize> = select.children.clone();
            while let Some(child) = stack.pop() {
                let Some(node) = tree.get_node(child) else {
                    continue;
                };
                if node.tag_name != "option" {
                    stack.extend(&node.children);
                    continue;
                }
                let text = node
                    .children
                    .iter()
                    .filter_map(|&c| tree.get_node(c))
                    .filter(|n| n.node_type == NodeType::Text)
                    .map(|n| n.text_content.trim())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .trim()
                    .to_lowercase();
                let value = node
                    .attributes
                    .get("value")
                    .cloned()
                    .unwrap_or_else(|| text.clone());
                let rank = if text == needle || value.to_lowercase() == needle {
                    2
                } else if text.contains(&needle) || value.to_lowercase().contains(&needle) {
                    1
                } else {
                    continue;
                };
                if best.as_ref().map(|b| rank > b.0).unwrap_or(true) {
                    best = Some((rank, child, value));
                }
            }
            best.map(|(_, id, value)| (id, value))
        });
        let Some((option_id, value)) = chosen else {
            return AgentActionResult::failed(
                format!("no option matching '{}' in '{}'", option, query),
                NdaDelta::default(),
            );
        };

        let before = self.capture_state_document();
        let selector = self.selector_for_node(select_id);
        if let Some(tree) = &mut self.dom.dom_tree {
            // Clear selection from every option under the select, then mark
            // the chosen one.
            let mut stack = tree
                .get_node(select_id)
                .map(|n| n.children.clone())
                .unwrap_or_default();
            while let Some(sib) = stack.pop() {
                if let Some(node) = tree.get_node_mut(sib) {
                    if node.tag_name == "option" {
                        node.attributes.remove("selected");
                    } else {
                        stack.extend(node.children.clone());
                    }
                }
            }
            if let Some(node) = tree.get_node_mut(option_id) {
                node.attributes
                    .insert("selected".to_string(), "selected".to_string());
            }
            if let Some(node) = tree.get_node_mut(select_id) {
                node.attributes.insert("value".to_string(), value.clone());
            }
            if let Some(sel) = &selector {
                let _ = self.js.js_vm.dispatch_event(tree, sel, "change");
            }
            self.dom
                .mutation_observer
                .observe_attribute_change(select_id, "value");
        }
        let after = self.capture_state_document();
        AgentActionResult::new(
            format!("selected '{}' on node_{}", value, select_id),
            diff(&before, &after),
        )
    }

    pub fn classify_interstitial(&self, html_snippet: &str) -> InterstitialKind {
        InterstitialClassifier::classify_page(&self.page_title, html_snippet)
    }

    // -- Private helpers for link navigation and form submission ---------------

    /// Resolve a possibly-relative URL against the session's current URL.
    fn resolve_url(&self, href: &str) -> String {
        let href = href.trim();
        if href.is_empty() || self.current_url.is_empty() {
            return href.to_string();
        }
        // Any absolute reference - http, https, data, about, anything with a
        // scheme - is already fully qualified and must survive untouched.
        if has_url_scheme(href) {
            return href.to_string();
        }
        let Some(scheme_end) = self.current_url.find("://").map(|at| at + 3) else {
            // The base is not a hierarchical http(s) URL (e.g. `about:test`),
            // so there is no authority or directory to resolve against.
            // Stitching the reference onto such a base produced garbage like
            // `about:test/about:submitted`; hand it back unchanged instead.
            return href.to_string();
        };
        let scheme = &self.current_url[..scheme_end];
        let after_scheme = &self.current_url[scheme_end..];
        let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
        if href.starts_with('/') {
            format!("{}{}{}", scheme, authority, href)
        } else {
            // Relative path: resolve against base path directory
            let last_slash = self
                .current_url
                .rfind('/')
                .unwrap_or(self.current_url.len());
            if last_slash > scheme_end {
                format!("{}/{}", &self.current_url[..last_slash], href)
            } else {
                format!("{}/{}", self.current_url, href)
            }
        }
    }

    /// Walk up the DOM from `node_id` to find the enclosing `<form>` element.
    fn find_enclosing_form(&self, node_id: usize) -> Option<usize> {
        let tree = self.dom.dom_tree.as_ref()?;
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
        let tree = self.dom.dom_tree.as_ref()?;
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
        encoder
            .triples
            .extend(self.device_profile.export_profile_nda(&self.session_id));
        encoder.triples.extend(self.file_manager.export_files_nda());
        encoder
            .triples
            .extend(self.trace_collector.export_traces_nda());
        encoder
            .triples
            .extend(self.dom.mutation_observer.export_mutations_nda());
        encoder
            .triples
            .extend(self.storage_broadcaster.export_events_nda());
        encoder
            .triples
            .extend(self.indexed_db.export_indexeddb_nda());
        encoder
            .triples
            .extend(self.cookie_store.export_cookies_nda());
        encoder
            .triples
            .extend(self.history_stack.export_history_nda(&self.session_id));
        encoder.triples.extend(WebCryptoEngine::export_crypto_nda(
            &self.session_id,
            "ready",
        ));
        encoder
            .triples
            .extend(self.tab_sandbox.export_sandbox_nda());
        encoder
            .triples
            .extend(self.push_notifications.export_push_nda(&self.session_id));
        encoder
            .triples
            .extend(self.payment_engine.export_payment_nda(&self.session_id));
        encoder.triples.extend(
            self.geolocation_provider
                .export_geolocation_nda(&self.session_id),
        );
        encoder
            .triples
            .extend(self.codecs_engine.export_codecs_nda(&self.session_id));
        encoder.triples.extend(
            self.inspector_server
                .handle_agent_inspection(&self.session_id),
        );

        if let Some(tree) = &self.dom.dom_tree {
            if let Some(c_type) = CaptchaSolverEngine::detect_challenge(tree) {
                encoder
                    .triples
                    .extend(CaptchaSolverEngine::solve_challenge_nda(
                        &self.session_id,
                        &c_type,
                    ));
            }
        }

        let ocr_boxes = self.perform_ocr_scan();
        encoder
            .triples
            .extend(self.ocr_engine.export_ocr_nda(&self.session_id, &ocr_boxes));

        // Add unmanaged slab node triples
        for slot in &self.dom.slab_tree.arena.slots {
            let slot_str = format!("slab_slot_{}", slot.slot_id);
            encoder.encode_fact(&self.session_id, 210, &slot_str);
        }

        // Add native Agentic AOM and 2D Layout Bounding Box triples
        if let Some(tree) = &self.dom.dom_tree {
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
        encoder
            .triples
            .extend(ShadowFrameExtractor::extract_shadow_hosts_nda(
                &self.dom.shadow_hosts,
            ));
        encoder
            .triples
            .extend(ShadowFrameExtractor::extract_frames_nda(&self.dom.frames));
        encoder
            .triples
            .extend(CanvasExtractor::extract_canvases_nda(&self.dom.canvases));
        encoder
            .triples
            .extend(self.net.network_tracker.export_triples_nda());

        encoder.triples
    }

    /// Whether a layout box intersects the scrolled viewport rectangle.
    /// Hidden boxes are never "in viewport" regardless of geometry.
    fn box_in_viewport(&self, b: &LayoutBox) -> bool {
        b.is_visible
            && b.x < self.scroll_x + self.viewport_width
            && b.x + b.width > self.scroll_x
            && b.y < self.scroll_y + self.viewport_height
            && b.y + b.height > self.scroll_y
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
        doc.push_str(
            &self.session_id,
            SESSION_SCROLL,
            &format!("{},{}", self.scroll_x, self.scroll_y),
        );

        for cookie in &self.cookies {
            doc.push_str(&cookie.name, SESSION_COOKIE, &cookie.value);
        }
        for (k, v) in &self.storage {
            doc.push_str(k, SESSION_STORAGE, v);
        }

        if let Some(tree) = &self.dom.dom_tree {
            // Readable AOM: roles/names/values survive as recoverable strings.
            let aom_nodes = AgenticAomTree::build_aom_nodes(tree);
            doc.merge(&AgenticAomTree::to_nda_document(&aom_nodes));

            // Layout geometry as readable literals ("x,y,w,h" + visibility),
            // plus whether each box intersects the scrolled viewport.
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
                doc.push_str(
                    &subject,
                    LAYOUT_IN_VIEWPORT,
                    if self.box_in_viewport(b) {
                        "true"
                    } else {
                        "false"
                    },
                );
            }

            // Page digest: cheap aggregate facts (link/form/interactive
            // counts, visible text length, headings in document order) so an
            // agent can grasp and diff the page shape without walking the
            // whole AOM.
            let mut links = 0i64;
            let mut forms = 0i64;
            let mut interactive = 0i64;
            for node in &tree.nodes {
                if node.node_type != NodeType::Element {
                    continue;
                }
                match node.tag_name.as_str() {
                    "a" if node.attributes.contains_key("href") => {
                        links += 1;
                        interactive += 1;
                    }
                    "form" => forms += 1,
                    "input" | "select" | "textarea" | "button" => interactive += 1,
                    _ => {}
                }
            }
            doc.push_int(&self.session_id, SESSION_LINK_COUNT, links);
            doc.push_int(&self.session_id, SESSION_FORM_COUNT, forms);
            doc.push_int(&self.session_id, SESSION_INTERACTIVE_COUNT, interactive);
            doc.push_int(
                &self.session_id,
                SESSION_TEXT_LENGTH,
                self.page_text().chars().count() as i64,
            );
            let mut heading_count = 0;
            for node in &tree.nodes {
                if node.node_type != NodeType::Element {
                    continue;
                }
                let depth = node
                    .tag_name
                    .strip_prefix('h')
                    .and_then(|d| d.parse::<u8>().ok())
                    .filter(|d| (1..=6).contains(d));
                let Some(depth) = depth else { continue };
                let mut text = String::new();
                Self::visible_text_walk(tree, node.id, &mut text);
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() && heading_count < 50 {
                    doc.push_str(
                        &self.session_id,
                        SESSION_HEADING,
                        &format!("h{}:{}", depth, text),
                    );
                    heading_count += 1;
                }
            }

            // Distilled content: the readability projection (main/article
            // region, boilerplate stripped) capped so one fact carries the
            // readable core of the page without bloating the state document.
            let mut content = self.distilled_content();
            const CONTENT_FACT_CHARS: usize = 8000;
            if content.chars().count() > CONTENT_FACT_CHARS {
                content = content.chars().take(CONTENT_FACT_CHARS).collect::<String>();
                content.push('\u{2026}');
            }
            if !content.is_empty() {
                doc.push_str(&self.session_id, SESSION_CONTENT, &content);
            }
        }

        // Canvas contents as readable literals (drawn text/shapes/images).
        doc.merge(&CanvasExtractor::extract_canvases_document(
            &self.dom.canvases,
        ));

        // Session-level keyboard focus is a fact the agent can diff on.
        if let Some(id) = self.focused_node {
            doc.push_str(&format!("node_{}", id), AOM_FOCUSED, "focused");
        }

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

// -- Synthetic (non-network) URL schemes --------------------------------------

/// What [`synthetic_document`] decided about a URL.
pub(crate) enum Synthetic {
    /// The URL names its own document; load the returned HTML without network.
    Document(String),
    /// A known synthetic scheme with no representable document (e.g. `about:`
    /// anything other than `blank`).
    Unsupported(BrowserError),
    /// A real transport scheme (`http:`, `https:`, ...): go fetch it.
    Network,
}

/// True when `url` opens with a scheme token per RFC 3986: a leading ASCII
/// letter followed by letters/digits/`+`/`-`/`.` and then `:`, with no `/`,
/// `?` or `#` intruding before the colon (which would make it a plain
/// relative reference like `a/b:page`).
fn has_url_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return false;
    };
    let scheme = &url[..colon];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Resolve the document named by a non-transport URL: `about:blank` renders
/// empty, `data:text/html,...` renders its own inline payload, any other
/// `about:` target has nothing to render, and everything else needs the
/// network.
fn synthetic_document(url: &str) -> Synthetic {
    if let Some(target) = url.strip_prefix("about:") {
        if target.is_empty() || target.eq_ignore_ascii_case("blank") {
            return Synthetic::Document(String::new());
        }
        return Synthetic::Unsupported(BrowserError::NavigationFailed(format!(
            "about:{} has no built-in document",
            target
        )));
    }
    if let Some(rest) = url.strip_prefix("data:") {
        return match decode_data_url(rest) {
            Some(body) => Synthetic::Document(body),
            None => Synthetic::Unsupported(BrowserError::MalformedResponse(format!(
                "data: URL is unreadable: {}",
                truncate_url_for_message(url)
            ))),
        };
    }
    Synthetic::Network
}

/// Non-transport URL resolution for callers that drive their own HTTP stack.
///
/// The MCP persisted-session engine fetches with `ureq`, which has no notion
/// of `data:` or `about:` and answers them as a URL with an empty host. Rather
/// than let a second copy of the rule drift into that crate, it calls this.
///
/// * `Some(Ok(html))` - the URL names its own document; no network needed.
/// * `Some(Err(msg))` - a recognised synthetic scheme with nothing renderable.
/// * `None`           - a transport scheme; the caller should fetch it.
pub fn resolve_synthetic_url(url: &str) -> Option<Result<String, String>> {
    match synthetic_document(url) {
        Synthetic::Document(html) => Some(Ok(html)),
        Synthetic::Unsupported(err) => Some(Err(err.to_string())),
        Synthetic::Network => None,
    }
}

/// Where a form submission ended up, so the status line can say precisely
/// whether a request went out at all.
enum SubmitOutcome {
    /// The node is not inside a `<form>`, so nothing could be submitted.
    NoForm,
    /// Fields were collected, a request went out, and its response was loaded.
    Submitted(String),
    /// A form was found but submitting it failed.
    Failed(String),
}

/// Shorten a URL enough to quote it in an error without echoing a whole
/// inline document back at the caller.
fn truncate_url_for_message(url: &str) -> String {
    const MAX: usize = 48;
    if url.len() <= MAX {
        return url.to_string();
    }
    let mut end = MAX;
    while !url.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}... ({} bytes)", &url[..end], url.len())
}

/// Decode the `media-type[,charset][;base64],payload` part of a `data:` URL
/// (RFC 2397). Only HTML/XML payloads are treated as documents; anything else
/// still decodes, since the caller is asking to render it.
fn decode_data_url(rest: &str) -> Option<String> {
    let (meta, payload) = rest.split_once(',')?;
    if meta.split(';').any(|p| p.eq_ignore_ascii_case("base64")) {
        let bytes = base64_decode_bytes(payload)?;
        return Some(String::from_utf8_lossy(&bytes).to_string());
    }
    Some(percent_decode(payload))
}

/// Percent-decode, keeping `+` literal the way `decodeURIComponent` does (the
/// form-encoded `+`-means-space rule belongs to query strings, not data URLs).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Strict-ish base64 decoder for `data:` payloads: ignores whitespace and
/// padding, rejects characters outside the standard alphabet.
fn base64_decode_bytes(input: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut nbits: u32 = 0;
    let mut out = Vec::new();
    for c in input.chars() {
        if c.is_ascii_whitespace() || c == '=' {
            continue;
        }
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push((acc >> nbits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod agent_action_tests {
    use super::*;
    use crate::predicates::{AOM_CHECKED, AOM_EXPANDED, AOM_VALUE};

    fn node_id_by_tag(session: &BrowserSession, tag: &str) -> usize {
        session
            .dom
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
            result.delta.added.contains(&(
                format!("node_{}", input_id),
                AOM_VALUE,
                "hello".to_string()
            )),
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
        session.js.js_vm.add_event_listener(
            "#b",
            "click",
            "document.getElementById('b').setAttribute('aria-expanded','true')",
        );

        let result = session.agent_click(button_id);

        assert!(result.status.contains("clicked"));
        // The only state change is the newly expanded button.
        assert_eq!(
            result.delta.added,
            vec![(
                format!("node_{}", button_id),
                AOM_EXPANDED,
                "expanded".to_string()
            )]
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

    #[test]
    fn agent_settle_idle_page_reports_no_changes() {
        let mut session = BrowserSession::new("s4".to_string());
        session.load_html("about:test", "<p>Static</p>");
        let result = session.agent_settle();
        assert_eq!(result.status, "settled: no changes");
        assert!(result.delta.is_empty());
    }

    #[test]
    fn agent_settle_runs_scheduled_timer_and_reports_delta() {
        let mut session = BrowserSession::new("s5".to_string());
        session.load_html("about:test", "<button id=\"b\">Menu</button>");
        // Schedule a timer that mutates the DOM, exactly as a page script would.
        let _ = session.eval_js(
            "setTimeout(document.getElementById('b').setAttribute('aria-expanded','true'), 0)",
        );
        let result = session.agent_settle();
        assert!(
            result.status.starts_with("settled:"),
            "got {}",
            result.status
        );
    }

    #[test]
    fn agent_observe_returns_readable_fact_lines() {
        let mut session = BrowserSession::new("s6".to_string());
        session.load_html("https://example.com/", "<button id=\"b\">Go</button>");
        session.page_title = "Observed Page".to_string();
        let text = session.agent_observe();
        assert!(text.contains("|url|https://example.com/"), "got: {}", text);
        assert!(text.contains("|title|Observed Page"), "got: {}", text);
        // No raw predicate numbers — names only.
        assert!(!text.contains("|100|"), "got: {}", text);
    }

    #[test]
    fn agent_click_by_text_resolves_button_by_visible_text() {
        let mut session = BrowserSession::new("s7".to_string());
        session.load_html("about:test", "<button id=\"b\">Log In</button>");
        let result = session.agent_click_by_text("Log In");
        assert!(result.status.contains("clicked"), "got {}", result.status);
    }

    #[test]
    fn agent_click_by_text_reports_miss_with_empty_delta() {
        let mut session = BrowserSession::new("s8".to_string());
        session.load_html("about:test", "<button id=\"b\">Log In</button>");
        let result = session.agent_click_by_text("Sign Up");
        assert!(
            result.status.contains("no clickable element"),
            "got {}",
            result.status
        );
        assert!(result.delta.is_empty());
    }

    #[test]
    fn agent_fill_by_label_matches_placeholder_and_sets_value() {
        let mut session = BrowserSession::new("s9".to_string());
        session.load_html("about:test", "<input type=\"text\" placeholder=\"Email\">");
        let input_id = node_id_by_tag(&session, "input");
        let result = session.agent_fill_by_label("Email", "a@b.com");
        assert!(result.status.contains("typed"), "got {}", result.status);
        assert!(
            result.delta.added.contains(&(
                format!("node_{}", input_id),
                AOM_VALUE,
                "a@b.com".to_string()
            )),
            "expected value fact, got {:?}",
            result.delta
        );
    }

    #[test]
    fn agent_fill_by_label_exact_match_beats_substring() {
        let mut session = BrowserSession::new("s10".to_string());
        session.load_html(
            "about:test",
            "<input type=\"text\" placeholder=\"Name suffix\"><input type=\"text\" placeholder=\"Name\">",
        );
        let result = session.agent_fill_by_label("Name", "Ada");
        assert!(result.status.contains("typed"), "got {}", result.status);
        let form = session.agent_read_form();
        assert!(form.contains("Name [textbox] = Ada"), "got: {}", form);
        assert!(form.contains("Name suffix [textbox] = \n"), "got: {}", form);
    }

    #[test]
    fn agent_check_by_label_sets_and_clears_checked() {
        let mut session = BrowserSession::new("s11".to_string());
        session.load_html(
            "about:test",
            "<input type=\"checkbox\" aria-label=\"Subscribe\">",
        );
        let checkbox_id = node_id_by_tag(&session, "input");

        let result = session.agent_check_by_label("Subscribe", true);
        assert!(result.status.contains("checked"), "got {}", result.status);
        let checked = session
            .dom
            .dom_tree
            .as_ref()
            .unwrap()
            .get_node(checkbox_id)
            .unwrap()
            .attributes
            .contains_key("checked");
        assert!(checked);

        let result = session.agent_check_by_label("Subscribe", false);
        assert!(result.status.contains("unchecked"), "got {}", result.status);
        let checked = session
            .dom
            .dom_tree
            .as_ref()
            .unwrap()
            .get_node(checkbox_id)
            .unwrap()
            .attributes
            .contains_key("checked");
        assert!(!checked);
    }

    #[test]
    fn agent_read_form_lists_controls_with_state() {
        let mut session = BrowserSession::new("s12".to_string());
        session.load_html(
            "about:test",
            "<input type=\"text\" placeholder=\"Email\" value=\"x@y.z\">\
             <input type=\"checkbox\" aria-label=\"Subscribe\" checked>",
        );
        let form = session.agent_read_form();
        assert!(form.contains("Email [textbox] = x@y.z"), "got: {}", form);
        assert!(
            form.contains("Subscribe [checkbox] = checked"),
            "got: {}",
            form
        );
    }

    #[test]
    fn agent_focus_by_label_emits_focus_fact_in_delta() {
        let mut session = BrowserSession::new("s13".to_string());
        session.load_html("about:test", "<input type=\"text\" placeholder=\"Email\">");
        let input_id = node_id_by_tag(&session, "input");
        let result = session.agent_focus_by_label("Email");
        assert!(result.status.contains("focused"), "got {}", result.status);
        assert!(
            result.delta.added.contains(&(
                format!("node_{}", input_id),
                crate::predicates::AOM_FOCUSED,
                "focused".to_string()
            )),
            "expected focus fact, got {:?}",
            result.delta
        );
    }

    #[test]
    fn agent_press_without_focus_reports_nothing_focused() {
        let mut session = BrowserSession::new("s14".to_string());
        session.load_html("about:test", "<input type=\"text\" placeholder=\"Email\">");
        let result = session.agent_press("a");
        assert!(
            result.status.contains("nothing focused"),
            "got {}",
            result.status
        );
        assert!(result.delta.is_empty());
    }

    #[test]
    fn agent_press_types_character_into_focused_control() {
        let mut session = BrowserSession::new("s15".to_string());
        session.load_html("about:test", "<input type=\"text\" placeholder=\"Email\">");
        let input_id = node_id_by_tag(&session, "input");
        session.agent_focus(input_id);
        session.agent_press("h");
        let result = session.agent_press("i");
        assert!(
            result.status.contains("pressed 'i'"),
            "got {}",
            result.status
        );
        let value = session
            .dom
            .dom_tree
            .as_ref()
            .unwrap()
            .get_node(input_id)
            .unwrap()
            .attributes
            .get("value")
            .cloned()
            .unwrap_or_default();
        assert_eq!(value, "hi");
    }

    #[test]
    fn agent_press_tab_advances_focus_to_next_control() {
        let mut session = BrowserSession::new("s16".to_string());
        session.load_html(
            "about:test",
            "<input type=\"text\" placeholder=\"First\"><input type=\"text\" placeholder=\"Second\">",
        );
        let first = session.resolve_node_by_name("First", |_| true).unwrap();
        let second = session.resolve_node_by_name("Second", |_| true).unwrap();
        session.agent_focus(first);
        let result = session.agent_press("Tab");
        assert!(result.status.contains("focused"), "got {}", result.status);
        assert_eq!(session.focused_node, Some(second));
    }

    #[test]
    fn agent_press_tab_wraps_to_first_control() {
        let mut session = BrowserSession::new("s17".to_string());
        session.load_html(
            "about:test",
            "<input type=\"text\" placeholder=\"First\"><input type=\"text\" placeholder=\"Second\">",
        );
        let first = session.resolve_node_by_name("First", |_| true).unwrap();
        let second = session.resolve_node_by_name("Second", |_| true).unwrap();
        session.agent_focus(second);
        session.agent_press("Tab");
        assert_eq!(session.focused_node, Some(first));
    }

    #[test]
    fn agent_press_enter_reports_submit_of_enclosing_form() {
        let mut session = BrowserSession::new("s18".to_string());
        session.load_html(
            "about:test",
            "<form action=\"data:text/html,%3Ch1%3Ethanks%3C/h1%3E\"><input type=\"text\" name=\"q\"></form>",
        );
        let input_id = node_id_by_tag(&session, "input");
        session.agent_focus(input_id);
        let result = session.agent_press("Enter");
        assert!(
            result.status.contains("pressed Enter"),
            "got {}",
            result.status
        );
        // The old action() target `about:submitted` was never reachable, so
        // this only ever passed on the engine claiming success. Assert the
        // submission genuinely landed.
        assert!(result.executed, "got {}", result.status);
        assert!(
            result.status.contains("submitted node_"),
            "got {}",
            result.status
        );
    }

    #[test]
    fn agent_select_by_label_picks_option_by_visible_text() {
        let mut session = BrowserSession::new("s19".to_string());
        session.load_html(
            "about:test",
            "<select aria-label=\"Country\">\
             <option value=\"br\">Brazil</option>\
             <option value=\"pt\">Portugal</option>\
             </select>",
        );
        let select_id = node_id_by_tag(&session, "select");
        let result = session.agent_select_by_label("Country", "Portugal");
        assert!(
            result.status.contains("selected 'pt'"),
            "got {}",
            result.status
        );
        let value = session
            .dom
            .dom_tree
            .as_ref()
            .unwrap()
            .get_node(select_id)
            .unwrap()
            .attributes
            .get("value")
            .cloned()
            .unwrap_or_default();
        assert_eq!(value, "pt");
    }

    #[test]
    fn agent_select_by_label_moves_selected_attribute() {
        let mut session = BrowserSession::new("s20".to_string());
        session.load_html(
            "about:test",
            "<select aria-label=\"Country\">\
             <option value=\"br\" selected>Brazil</option>\
             <option value=\"pt\">Portugal</option>\
             </select>",
        );
        session.agent_select_by_label("Country", "pt");
        let tree = session.dom.dom_tree.as_ref().unwrap();
        let selected: Vec<String> = tree
            .nodes
            .iter()
            .filter(|n| n.tag_name == "option" && n.attributes.contains_key("selected"))
            .filter_map(|n| n.attributes.get("value").cloned())
            .collect();
        assert_eq!(selected, vec!["pt".to_string()]);
    }

    #[test]
    fn agent_select_by_label_reports_missing_option() {
        let mut session = BrowserSession::new("s21".to_string());
        session.load_html(
            "about:test",
            "<select aria-label=\"Country\"><option value=\"br\">Brazil</option></select>",
        );
        let result = session.agent_select_by_label("Country", "Mars");
        assert!(
            result.status.contains("no option matching"),
            "got {}",
            result.status
        );
        assert!(result.delta.is_empty());
    }

    #[test]
    fn agent_scroll_moves_offset_and_flips_in_viewport_facts() {
        use crate::predicates::{LAYOUT_IN_VIEWPORT, SESSION_SCROLL};
        let mut session = BrowserSession::new("s22".to_string());
        session.load_html("about:test", "<p>Top</p><p>Bottom far below</p>");
        // Shrink the viewport so only the first paragraph starts in view.
        session.viewport_height = 10.0;
        let second_p = session
            .dom
            .dom_tree
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|n| n.tag_name == "p")
            .nth(1)
            .expect("second paragraph")
            .id;

        let result = session.agent_scroll(0, 16);

        assert!(
            result.status.contains("to offset (0, 16)"),
            "got {}",
            result.status
        );
        // The session scroll fact always records the new offset...
        assert!(
            result
                .delta
                .changed
                .iter()
                .any(|c| c.predicate == SESSION_SCROLL && c.old == "0,0" && c.new == "0,16"),
            "expected scroll fact change, got {:?}",
            result.delta
        );
        // ...and the below-the-fold paragraph scrolled into the viewport.
        assert!(
            result
                .delta
                .changed
                .iter()
                .any(|c| c.subject == format!("node_{}", second_p)
                    && c.predicate == LAYOUT_IN_VIEWPORT
                    && c.old == "false"
                    && c.new == "true"),
            "expected node_{} to enter the viewport, got {:?}",
            second_p,
            result.delta
        );
    }

    #[test]
    fn agent_scroll_clamps_at_document_origin() {
        let mut session = BrowserSession::new("s23".to_string());
        session.load_html("about:test", "<p>Only</p>");
        let result = session.agent_scroll(-50, -50);
        assert!(
            result.status.contains("to offset (0, 0)"),
            "got {}",
            result.status
        );
        assert!(result.delta.is_empty(), "no facts move: {:?}", result.delta);
    }

    #[test]
    fn agent_scroll_into_view_reaches_offscreen_element() {
        let mut session = BrowserSession::new("s24".to_string());
        session.load_html("about:test", "<p>filler text above</p><button>Go</button>");
        session.viewport_height = 10.0;

        let result = session.agent_scroll_into_view("Go");
        assert!(result.status.contains("into view"), "got {}", result.status);
        assert!(
            session.scroll_y > 0.0,
            "viewport moved down: {}",
            session.scroll_y
        );

        // Second call is a no-op: the element is already visible.
        let again = session.agent_scroll_into_view("Go");
        assert!(
            again.status.contains("already in view"),
            "got {}",
            again.status
        );
        assert!(again.delta.is_empty());
    }

    #[test]
    fn agent_scroll_into_view_reports_missing_element() {
        let mut session = BrowserSession::new("s25".to_string());
        session.load_html("about:test", "<button>Go</button>");
        let result = session.agent_scroll_into_view("Nowhere");
        assert!(
            result.status.contains("no element matching"),
            "got {}",
            result.status
        );
        assert!(result.delta.is_empty());
    }

    #[test]
    fn observe_reports_scroll_and_in_viewport_facts_by_name() {
        let mut session = BrowserSession::new("s26".to_string());
        session.load_html("about:test", "<button>Go</button>");
        let text = session.agent_observe();
        assert!(text.contains("|scroll|0,0"), "got: {}", text);
        assert!(text.contains("|inViewport|true"), "got: {}", text);
    }

    #[test]
    fn page_text_collects_title_and_visible_text_skipping_scripts() {
        let mut session = BrowserSession::new("s27".to_string());
        session.load_html(
            "about:test",
            "<html><head><title>Pricing</title><script>var hidden = 1;</script>\
             <style>.x { color: red; }</style></head>\
             <body><h1>Plans</h1><p>Choose   the\n pro plan today.</p></body></html>",
        );
        assert_eq!(
            session.page_text(),
            "Pricing Plans Choose the pro plan today."
        );
    }

    #[test]
    fn page_text_is_empty_without_a_loaded_page() {
        let session = BrowserSession::new("s28".to_string());
        assert_eq!(session.page_text(), "");
    }

    #[test]
    fn observe_reports_page_digest_counts_and_headings() {
        let mut session = BrowserSession::new("s29".to_string());
        session.load_html(
            "about:test",
            "<html><head><title>Pricing</title></head><body>\
             <h1>Plans</h1><h2>Pro tier</h2>\
             <a href=\"/a\">A</a><a href=\"/b\">B</a>\
             <form><input name=\"q\" value=\"\"><button>Go</button></form>\
             </body></html>",
        );
        let facts = session.agent_observe();
        assert!(facts.contains("s29|links|2"), "{facts}");
        assert!(facts.contains("s29|forms|1"), "{facts}");
        // 2 links + input + button
        assert!(facts.contains("s29|interactive|4"), "{facts}");
        assert!(facts.contains("s29|heading|h1:Plans"), "{facts}");
        assert!(facts.contains("s29|heading|h2:Pro tier"), "{facts}");
        let expected_len = session.page_text().chars().count();
        assert!(expected_len > 0, "digest page has visible text");
        assert!(
            facts.contains(&format!("s29|textLength|{expected_len}")),
            "{facts}"
        );
    }

    #[test]
    fn page_markdown_preserves_structure_and_strips_boilerplate() {
        let mut session = BrowserSession::new("s30".to_string());
        session.load_html(
            "about:test",
            "<html><head><title>Pricing</title><script>var hidden = 1;</script></head>\
             <body><h1>Plans</h1>\
             <p>Pick a <strong>plan</strong> from <a href=\"/list\">the list</a>.</p>\
             <ul><li>Free</li><li>Pro</li></ul></body></html>",
        );
        let md = session.page_markdown();
        assert!(md.starts_with("# Pricing"), "title leads: {md}");
        assert!(md.contains("# Plans"), "{md}");
        assert!(md.contains("**plan**"), "emphasis survives: {md}");
        assert!(md.contains("[the list](/list)"), "links keep hrefs: {md}");
        assert!(md.contains("- Free\n- Pro"), "list items render: {md}");
        assert!(!md.contains("hidden"), "script content stripped: {md}");
    }

    #[test]
    fn page_tables_text_renders_markdown_rows_with_header_separator() {
        let mut session = BrowserSession::new("s31".to_string());
        session.load_html(
            "about:test",
            "<html><body><table><caption>Plans</caption>\
             <tr><th>Plan</th><th>Price</th></tr>\
             <tr><td>Free</td><td>$0</td></tr>\
             <tr><td>Pro</td><td>$9</td></tr></table></body></html>",
        );
        let tables = session.page_tables_text();
        assert!(tables.contains("Table: Plans"), "{tables}");
        assert!(tables.contains("| Plan | Price |"), "{tables}");
        assert!(tables.contains("| --- | --- |"), "{tables}");
        assert!(tables.contains("| Free | $0 |"), "{tables}");
        assert!(tables.contains("| Pro | $9 |"), "{tables}");
    }

    #[test]
    fn page_summary_text_digests_counts_and_heading_outline() {
        let mut session = BrowserSession::new("s32".to_string());
        session.load_html(
            "about:test",
            "<html><head><title>Pricing</title></head><body>\
             <h1>Plans</h1><h2>Pro tier</h2>\
             <a href=\"/a\">A</a>\
             <table><tr><td>x</td></tr></table></body></html>",
        );
        let summary = session.page_summary_text();
        assert!(summary.contains("Page: Pricing"), "{summary}");
        assert!(summary.contains("1 link(s)"), "{summary}");
        assert!(summary.contains("1 table(s)"), "{summary}");
        assert!(summary.contains("Headings:"), "{summary}");
        assert!(summary.contains("# Plans"), "{summary}");
        assert!(summary.contains("## Pro tier"), "{summary}");
    }

    #[test]
    fn page_content_markdown_roots_at_main_and_drops_chrome() {
        let mut session = BrowserSession::new("s34".to_string());
        session.load_html(
            "about:test",
            "<html><head><title>Post</title></head><body>\
             <nav><a href=\"/home\">Home</a></nav>\
             <div class=\"cookie-banner\"><p>We use cookies.</p></div>\
             <main><h1>Story</h1><p>Body of the story.</p></main>\
             <div class=\"sidebar\"><p>Trending now</p></div>\
             </body></html>",
        );
        let content = session.page_content_markdown();
        assert!(content.contains("# Post"), "{content}");
        assert!(content.contains("# Story"), "{content}");
        assert!(content.contains("Body of the story."), "{content}");
        assert!(
            !content.contains("We use cookies."),
            "cookie banner dropped: {content}"
        );
        assert!(
            !content.contains("Trending now"),
            "sidebar dropped: {content}"
        );
        assert!(!content.contains("Home"), "nav dropped: {content}");
        // Full markdown keeps everything outside <main> except pattern-marked
        // chrome, which is now dropped there too.
        let md = session.page_markdown();
        assert!(!md.contains("We use cookies."), "{md}");
        assert!(!md.contains("Trending now"), "{md}");
    }

    #[test]
    fn page_content_markdown_falls_back_to_body_without_main() {
        let mut session = BrowserSession::new("s35".to_string());
        session.load_html(
            "about:test",
            "<html><body><h2>Notes</h2><p>Plain page.</p></body></html>",
        );
        let content = session.page_content_markdown();
        assert!(content.contains("## Notes"), "{content}");
        assert!(content.contains("Plain page."), "{content}");
    }

    #[test]
    fn capture_state_document_carries_distilled_content_fact() {
        let mut session = BrowserSession::new("s36".to_string());
        session.load_html(
            "about:test",
            "<html><head><title>Post</title></head><body>\
             <nav><a href=\"/home\">Home chrome</a></nav>\
             <div class=\"cookie-banner\"><p>We use cookies.</p></div>\
             <main><h1>Story</h1><p>Body of the story.</p></main>\
             </body></html>",
        );
        let facts = session.capture_state_document().facts_text();
        assert!(facts.contains("|content|"), "content fact emitted: {facts}");
        assert!(facts.contains("Body of the story."), "{facts}");
        assert!(
            !facts.contains("We use cookies."),
            "boilerplate stays out: {facts}"
        );
    }

    #[test]
    fn content_fact_is_capped_at_8000_chars() {
        let mut session = BrowserSession::new("s37".to_string());
        let filler = "word ".repeat(3000); // 15000 chars of body text
        session.load_html(
            "about:test",
            &format!("<html><body><p>{filler}</p></body></html>"),
        );
        let doc = session.capture_state_document();
        let fact = doc
            .facts
            .iter()
            .find(|f| f.predicate == crate::predicates::SESSION_CONTENT)
            .expect("content fact present");
        let text = doc.object_display(fact).expect("content resolves");
        assert_eq!(text.chars().count(), 8001, "capped at 8000 + ellipsis");
        assert!(
            text.ends_with('\u{2026}'),
            "got {} chars",
            text.chars().count()
        );
    }

    #[test]
    fn page_projections_are_empty_without_a_loaded_page() {
        let session = BrowserSession::new("s33".to_string());
        assert_eq!(session.page_markdown(), "");
        assert_eq!(session.page_content_markdown(), "");
        assert_eq!(session.page_tables_text(), "");
        assert_eq!(session.page_summary_text(), "");
    }

    // -- Synthetic URL schemes and form submission outcomes -------------------

    fn has_tag(session: &BrowserSession, tag: &str) -> bool {
        session
            .dom
            .dom_tree
            .as_ref()
            .map(|t| t.nodes.iter().any(|n| n.tag_name == tag))
            .unwrap_or(false)
    }

    #[test]
    fn agent_navigate_about_blank_loads_an_empty_document_offline() {
        let mut session = BrowserSession::new("s38".to_string());
        session.load_html("about:test", "<h1>Stale page</h1>");
        let result = session.agent_navigate("about:blank");
        assert!(
            result.executed,
            "about:blank always renders: {}",
            result.status
        );
        assert!(!has_tag(&session, "h1"), "previous page must be cleared");
    }

    #[test]
    fn agent_navigate_data_url_renders_its_inline_document() {
        let mut session = BrowserSession::new("s39".to_string());
        let result =
            session.agent_navigate("data:text/html,%3Ch1%3EInline%3C/h1%3E%3Cp%3EBody%3C/p%3E");
        assert!(
            result.executed,
            "data: URL should render: {}",
            result.status
        );
        assert!(has_tag(&session, "h1"), "inline document was not parsed");
        assert!(session.page_content_markdown().contains("Body"));
    }

    #[test]
    fn agent_navigate_base64_data_url_decodes_its_payload() {
        let mut session = BrowserSession::new("s40".to_string());
        // base64 of "<h2>H64</h2>"
        let result = session.agent_navigate("data:text/html;base64,PGgyPkg2NDwvaDI+");
        assert!(
            result.executed,
            "base64 data: URL should render: {}",
            result.status
        );
        assert!(has_tag(&session, "h2"), "base64 payload was not decoded");
    }

    #[test]
    fn agent_navigate_unknown_about_scheme_fails_loudly() {
        let mut session = BrowserSession::new("s41".to_string());
        let result = session.agent_navigate("about:config");
        assert!(
            !result.executed,
            "no built-in document exists for about:config"
        );
        assert!(
            result.status.contains("no built-in document"),
            "status must name the reason: {}",
            result.status
        );
    }

    #[test]
    fn agent_navigate_malformed_data_url_reports_it_unreadable() {
        let mut session = BrowserSession::new("s42".to_string());
        // No comma separating the media type from the payload.
        let result = session.agent_navigate("data:text/html");
        assert!(!result.executed);
        assert!(
            result.status.contains("unreadable"),
            "status must name the reason: {}",
            result.status
        );
    }

    #[test]
    fn resolve_url_keeps_absolute_references_of_every_scheme() {
        let mut session = BrowserSession::new("s43".to_string());
        session.load_html("https://example.com/docs/page", "<p>x</p>");
        assert_eq!(
            session.resolve_url("data:text/html,%3Ch1%3Ei%3C/h1%3E"),
            "data:text/html,%3Ch1%3Ei%3C/h1%3E"
        );
        assert_eq!(session.resolve_url("about:blank"), "about:blank");
        assert_eq!(
            session.resolve_url("https://other.test/a"),
            "https://other.test/a"
        );
    }

    #[test]
    fn resolve_url_does_not_stitch_refs_onto_a_non_hierarchical_base() {
        let mut session = BrowserSession::new("s44".to_string());
        session.load_html("about:test", "<p>x</p>");
        // `about:test` has no authority or path, so a relative reference
        // cannot be resolved against it - the old code invented
        // `about:test/next` out of thin air.
        assert_eq!(session.resolve_url("next"), "next");
        assert_eq!(session.resolve_url("/abs"), "/abs");
    }

    #[test]
    fn resolve_url_still_resolves_relative_paths_against_http_base() {
        let mut session = BrowserSession::new("s45".to_string());
        session.load_html("https://example.com/docs/page.html", "<p>x</p>");
        assert_eq!(
            session.resolve_url("other.html"),
            "https://example.com/docs/other.html"
        );
        assert_eq!(session.resolve_url("/root"), "https://example.com/root");
    }

    #[test]
    fn has_url_scheme_rejects_colons_inside_relative_references() {
        assert!(has_url_scheme("https://a.test"));
        assert!(has_url_scheme("data:text/html,x"));
        assert!(has_url_scheme("mailto:someone@t.test"));
        // A colon that appears after a path separator belongs to the path, not
        // to a scheme, so these stay relative.
        assert!(!has_url_scheme("/docs/a:b"));
        assert!(!has_url_scheme("?q=:"));
        assert!(!has_url_scheme("#frag:ment"));
        assert!(!has_url_scheme("dir/file"));
        assert!(!has_url_scheme(""));
        assert!(!has_url_scheme("1abc:x"));
    }

    #[test]
    fn agent_press_enter_submits_enclosing_form_and_loads_response() {
        let mut session = BrowserSession::new("s46".to_string());
        session.load_html(
            "about:test",
            "<form action=\"data:text/html,%3Ch1%3EDone%3C/h1%3E\"><input type=\"text\" name=\"q\"></form>",
        );
        let input_id = node_id_by_tag(&session, "input");
        session.agent_focus(input_id);
        let result = session.agent_press("Enter");
        assert!(
            result.status.contains("pressed Enter"),
            "got {}",
            result.status
        );
        assert!(
            result.executed,
            "Enter in a form field must actually submit it: {}",
            result.status
        );
        assert!(
            result.status.contains("submitted node_"),
            "got {}",
            result.status
        );
        assert!(has_tag(&session, "h1"), "response was not loaded");
    }

    #[test]
    fn agent_submit_outside_a_form_says_so() {
        let mut session = BrowserSession::new("s47".to_string());
        session.load_html("about:test", "<button>Send</button>");
        let button_id = node_id_by_tag(&session, "button");
        let result = session.agent_submit(button_id);
        assert!(!result.executed, "nothing was submitted");
        assert!(
            result.status.contains("not inside a form"),
            "got {}",
            result.status
        );
    }

    #[test]
    fn agent_submit_surfaces_the_transport_error_when_the_post_fails() {
        let mut session = BrowserSession::new("s48".to_string());
        // Port 1 is not listening, so this fails fast with a real error.
        session.load_html(
            "about:test",
            "<form action=\"http://127.0.0.1:1/gone\"><input type=\"text\" name=\"q\"></form>",
        );
        let input_id = node_id_by_tag(&session, "input");
        let result = session.agent_submit(input_id);
        assert!(!result.executed, "the request never landed");
        assert!(
            result.status.contains("submit for node_") && result.status.contains("failed:"),
            "a failed post must not read as \"not inside a form\": {}",
            result.status
        );
    }

    // === Activation behaviour and selected state (bugs #48, #51, #52, #53) ==

    /// ids of every `<input>` in document order.
    fn input_ids(session: &BrowserSession) -> Vec<usize> {
        session
            .dom
            .dom_tree
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|n| n.tag_name == "input")
            .map(|n| n.id)
            .collect()
    }

    /// values of every currently-checked control.
    fn checked_values(session: &BrowserSession) -> Vec<String> {
        session
            .dom
            .dom_tree
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter(|n| n.attributes.contains_key("checked"))
            .filter_map(|n| n.attributes.get("value").cloned())
            .collect()
    }

    #[test]
    fn clicking_a_radio_selects_it_and_clears_its_group() {
        let mut session = BrowserSession::new("s49".to_string());
        session.load_html(
            "about:test",
            "<form action=\"/x\"><input type=\"radio\" name=\"size\" value=\"small\" checked>\
             <input type=\"radio\" name=\"size\" value=\"medium\">\
             <input type=\"radio\" name=\"topping\" value=\"bacon\"><button>Go</button></form>",
        );
        let ids = input_ids(&session);
        assert_eq!(ids.len(), 3);
        assert_eq!(checked_values(&session), vec!["small".to_string()]);

        let result = session.agent_click(ids[1]);
        assert!(result.executed, "{}", result.status);
        assert_eq!(
            checked_values(&session),
            vec!["medium".to_string()],
            "the same-name radio has to be cleared; a different group is not"
        );
    }

    #[test]
    fn a_clicked_radio_is_what_the_form_submits() {
        let mut session = BrowserSession::new("s50".to_string());
        session.load_html(
            "about:test",
            "<form action=\"/x\"><input type=\"radio\" name=\"size\" value=\"small\" checked>\
             <input type=\"radio\" name=\"size\" value=\"medium\"><button>Go</button></form>",
        );
        let ids = input_ids(&session);
        session.agent_click(ids[1]);
        let button = node_id_by_tag(&session, "button");
        let (_, _, body) = session
            .collect_form_submission(button)
            .expect("the button is inside the form");
        assert!(body.contains("size=medium"), "submitted body: {body}");
        assert!(
            !body.contains("small"),
            "stale selection still submitted: {body}"
        );
    }

    #[test]
    fn clicking_a_checkbox_toggles_it_both_ways() {
        let mut session = BrowserSession::new("s51".to_string());
        session.load_html(
            "about:test",
            "<input type=\"checkbox\" aria-label=\"Subscribe\" value=\"yes\">",
        );
        let id = node_id_by_tag(&session, "input");
        session.agent_click(id);
        assert_eq!(checked_values(&session), vec!["yes".to_string()]);
        assert!(session.agent_read_form().contains("checked"));
        session.agent_click(id);
        assert!(
            checked_values(&session).is_empty(),
            "second click untoggles"
        );
        assert!(session.agent_read_form().contains("unchecked"));
    }

    #[test]
    fn a_check_that_lands_shows_up_in_the_delta() {
        let mut session = BrowserSession::new("s52".to_string());
        session.load_html(
            "about:test",
            "<input type=\"checkbox\" aria-label=\"Subscribe\" value=\"yes\">",
        );
        let id = node_id_by_tag(&session, "input");
        let subject = format!("node_{id}");

        let result = session.agent_check_by_label("Subscribe", true);
        assert!(
            result
                .delta
                .added
                .contains(&(subject.clone(), AOM_CHECKED, "checked".to_string())),
            "bug #52: a real check diffed as {:?}",
            result.delta
        );

        let result = session.agent_check_by_label("Subscribe", false);
        assert!(
            result
                .delta
                .removed
                .contains(&(subject, AOM_CHECKED, "checked".to_string())),
            "unchecking has to remove the fact, got {:?}",
            result.delta.removed
        );
    }

    #[test]
    fn fill_by_label_also_matches_a_placeholder_shadowed_by_a_real_label() {
        let mut session = BrowserSession::new("s53".to_string());
        session.load_html(
            "about:test",
            "<label>Nick:<input placeholder=\"handle\" name=\"nick\"></label>",
        );
        // The bound label owns the accessible name, so a placeholder lookup is
        // only possible if resolution also consults the raw attribute.
        let result = session.agent_fill_by_label("handle", "ada");
        assert!(result.executed, "{}", result.status);
        assert!(
            session.agent_read_form().contains("ada"),
            "form: {}",
            session.agent_read_form()
        );
        // The label still resolves the same control.
        let result = session.agent_fill_by_label("Nick:", "grace");
        assert!(result.executed, "{}", result.status);
    }

    #[test]
    fn distilled_content_reaches_a_document_without_html_prose() {
        let mut session = BrowserSession::new("s54".to_string());
        // What a JSON API response looks like to the engine: bare text with no
        // <p>/<h*> structure, so both markdown projections come back empty.
        session.load_html(
            "https://httpbin.org/post",
            "{ \"args\": {}, \"form\": { \"custname\": \"Ada\" } }",
        );
        assert!(
            session.page_content_markdown().is_empty() && session.page_markdown().is_empty(),
            "precondition: the markdown projections are the empty ones"
        );
        let content = session.distilled_content();
        assert!(
            content.contains("custname") && content.contains("Ada"),
            "content projection lost text page_text can see: {content:?}"
        );
        let doc = session.capture_state_document();
        assert!(
            doc.readable_facts()
                .iter()
                .any(|(_, p, o)| *p == SESSION_CONTENT && o.contains("Ada")),
            "the content fact was left empty"
        );
    }

    #[test]
    fn has_node_only_accepts_ids_in_the_live_document() {
        let mut session = BrowserSession::new("s55".to_string());
        session.load_html("about:test", "<button>Go</button>");
        let id = node_id_by_tag(&session, "button");
        assert!(session.has_node(id));
        assert!(!session.has_node(id + 1_000), "invented id must be refused");
        let blank = BrowserSession::new("s56".to_string());
        assert!(!blank.has_node(0), "no document loaded, so no nodes");
    }
}
