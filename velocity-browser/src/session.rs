use crate::agentic::{AgenticAomTree, NdaEncoder};
use crate::dom::{DomTree, MutationBatcher, NativeMutationObserver};
use crate::engine::{
    Canvas2DContext, CanvasElement, CanvasExtractor, ConsoleTraceRecord, DeviceProfile, DownloadStreamArtifact, FileChooserEvent,
    FileManager, FrameTarget, InterstitialClassifier, InterstitialKind, NetworkTracker, PixelBuffer,
    ShadowFrameExtractor, ShadowHost, SoftwareRasterizer, SvgVectorEngine, TraceCollector,
};
use crate::js::{JsEventLoopScheduler, JsVirtualMachine};
use crate::layout::{DisplayMode, FlexDirection, FlexLayoutEngine, LayoutBox, LayoutEngine2D};
use crate::net::{HttpClient, NativeWsClient, ProxyResolver, WebRtcTransport};
use crate::nda::NdaTriple;
use crate::parser::{CssMatcher, HtmlParser, Html5Tokenizer};
use crate::session_auth::{AuthReseeder, AuthTokenState};
use crate::session_indexeddb::IndexedDbStorage;
use crate::session_storage_events::{StorageEventBroadcaster, StorageEventRecord};
use crate::style::StyleCascader;
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
    pub http_client: HttpClient,
    pub network_tracker: NetworkTracker,
    pub file_manager: FileManager,
    pub device_profile: DeviceProfile,
    pub trace_collector: TraceCollector,
    pub mutation_observer: NativeMutationObserver,
    pub mutation_batcher: MutationBatcher,
    pub storage_broadcaster: StorageEventBroadcaster,
    pub indexed_db: IndexedDbStorage,
    pub proxy_resolver: ProxyResolver,
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
            http_client: HttpClient::new(),
            network_tracker: NetworkTracker::new(),
            file_manager: FileManager::new(),
            device_profile: DeviceProfile::desktop_chrome(),
            trace_collector: TraceCollector::new(),
            mutation_observer: NativeMutationObserver::new(),
            mutation_batcher: MutationBatcher::new(),
            storage_broadcaster: StorageEventBroadcaster::new(),
            indexed_db: IndexedDbStorage::new(&format!("db_{}", session_id)),
            proxy_resolver: ProxyResolver::direct(),
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

    /// Fetch HTML over native HTTP transport client and parse into DOM tree
    pub fn fetch_and_load(&mut self, url: &str) -> Result<Vec<NdaTriple>, Box<dyn std::error::Error + Send + Sync>> {
        let resp = self.http_client.get(url)?;
        self.network_tracker.record_request(url, "GET", resp.status_code, "document");
        Ok(self.load_html(url, &resp.body))
    }

    /// Native pure-Rust HTML document loading and DOM tree compilation
    pub fn load_html(&mut self, url: &str, html: &str) -> Vec<NdaTriple> {
        self.current_url = url.to_string();
        let _tokens = Html5Tokenizer::new(html).tokenize();
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
                let _ = self.js_vm.dispatch_event(tree, selector, "click");
                self.mutation_observer.observe_attribute_change(node_id, "click");
                self.trace_collector.record_mutation(selector, "click", "Native click event dispatched");
                return Ok(());
            }
            return Err(format!("Element with selector '{}' not found", selector).into());
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
                    node.attributes.insert("value".to_string(), text.to_string());
                    let _ = self.js_vm.dispatch_event(tree, selector, "input");
                    self.mutation_observer.observe_attribute_change(id, "value");
                    self.trace_collector.record_mutation(selector, "attribute_changed", &format!("value={}", text));
                    return Ok(());
                }
            }
            return Err(format!("Element with selector '{}' not found", selector).into());
        }
        Err("No DOM tree loaded in session".into())
    }

    pub fn set_storage_item(&mut self, key: &str, value: &str) {
        self.storage_broadcaster.set_item(&mut self.storage, key, value, &self.current_url);
    }

    pub fn reseed_auth(&mut self, auth: &AuthTokenState) {
        AuthReseeder::reseed_into_session(self, auth);
    }

    pub fn attach_file(&mut self, selector: &str, file_path: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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

        // Add device profile, file, mutation, storage event, indexeddb, and trace triples
        encoder.triples.extend(self.device_profile.export_profile_nda(&self.session_id));
        encoder.triples.extend(self.file_manager.export_files_nda());
        encoder.triples.extend(self.trace_collector.export_traces_nda());
        encoder.triples.extend(self.mutation_observer.export_mutations_nda());
        encoder.triples.extend(self.storage_broadcaster.export_events_nda());
        encoder.triples.extend(self.indexed_db.export_indexeddb_nda());

        // Add native Agentic AOM and 2D Layout Bounding Box triples
        if let Some(tree) = &self.dom_tree {
            let aom_nodes = AgenticAomTree::build_aom_nodes(tree);
            for t in AgenticAomTree::to_nda_triples(&aom_nodes) {
                encoder.triples.push(t);
            }

            let layout_engine = LayoutEngine2D::new(self.cascader.clone());
            let mut boxes = layout_engine.build_layout_tree(tree);
            let root_box = LayoutBox {
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
