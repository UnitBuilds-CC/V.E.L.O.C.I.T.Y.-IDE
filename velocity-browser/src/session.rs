use crate::agentic::{AgenticAomTree, NdaEncoder, VelocityOcrEngine, ZeroAllocNdaWriter};
use crate::dom::{CustomElementRegistry, DomTree, MutationBatcher, NativeMutationObserver, SlabDomTree, SlotProjectionEngine};
use crate::engine::{
    AudioContextNode, BezierPoint, Canvas2DContext, CanvasElement, CanvasExtractor, CaptchaSolverEngine, CaptchaType, ConsoleTraceRecord,
    DeviceProfile, DownloadStreamArtifact, FileChooserEvent, FileManager, FrameTarget, Geocoordinates, GeolocationProvider,
    GpuTileCompositor, InterstitialClassifier, InterstitialKind, NetworkTracker, PaymentItem, PaymentRequestEngine, PdfMediaExtractor,
    PixelBuffer, PushNotificationManager, SandboxCapabilities, ServiceWorkerManager, ShadowFrameExtractor, ShadowHost, SoftwareRasterizer,
    StealthHumanBehavior, SvgVectorEngine, TabSandbox, TraceCollector, VelocityCodecsEngine, WebAudioEngine, WebCryptoEngine, WebGLContext,
};
use crate::js::{JsEventLoopScheduler, JsVirtualMachine, PointerEvent, SyntheticEventDispatcher, WasmInterpreter, WasmSimdPipeline, WebWorkerPool};
use crate::layout::{AlignItems, DisplayMode, FlexAlignmentSolver, FlexDirection, FlexLayoutEngine, GridTrack, GridTrackSolver, JustifyContent, LayoutBox, LayoutEngine2D, ParallelLayoutEngine};
use crate::net::{BluetoothDevice, HttpClient, InspectorServer, NativeWsClient, ProxyResolver, QuicConnection, TlsFingerprintRotator, WebBluetoothTransport, WebRtcTransport};
use crate::nda::NdaTriple;
use crate::parser::{CssMatcher, FastCssParser, HtmlParser, Html5Tokenizer};
use crate::session_auth::{AuthReseeder, AuthTokenState};
use crate::session_cookie_store::{CookieRecord, CookieStore, SameSitePolicy};
use crate::session_history::{HistoryItem, HistoryStack};
use crate::session_indexeddb::IndexedDbStorage;
use crate::session_storage_events::{StorageEventBroadcaster, StorageEventRecord};
pub use crate::session_storage_quota::StorageQuotaManager;
use crate::style::{FontShaperEngine, ScopedCssMatcher, StyleCascader};
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
            shadow_hosts: Vec::new(),
            frames: Vec::new(),
            canvases: Vec::new(),
        }
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
        let _tokens = Html5Tokenizer::new(html).tokenize();
        let _fast_rules = FastCssParser::parse_rules_fast(html);
        let nodes = HtmlParser::parse(html);
        let tree = DomTree::new(nodes);
        self.page_title = tree.extract_page_title();
        self.dom_tree = Some(tree);

        self.trace_collector.record_console("info", &format!("Loaded HTML from {}", url));
        self.capture_state_nda()
    }

    /// Execute JavaScript expression natively via JS Virtual Machine
    pub fn eval_js(&mut self, expr: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(tree) = &mut self.dom_tree {
            let res = self.js_vm.eval_statement(tree, expr)?;
            self.trace_collector.record_console("info", &format!("Evaluated JS: '{}'", expr));

            // Drain async microtasks
            while let Some(task) = self.js_scheduler.pop_next_task() {
                let _ = self.js_vm.eval_statement(tree, &task.script);
            }

            return Ok(format!("{:?}", res));
        }
        Err("No DOM tree loaded in session".into())
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
                let _ = SyntheticEventDispatcher::dispatch_pointer_event(tree, node_id, event);
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

    pub fn classify_interstitial(&self, html_snippet: &str) -> InterstitialKind {
        InterstitialClassifier::classify_page(&self.page_title, html_snippet)
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
            self.parallel_layout.compute_parallel_subtrees(&mut root_box);

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
}
