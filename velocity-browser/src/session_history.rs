use crate::nda::NdaTriple;

#[derive(Debug, Clone)]
pub struct HistoryItem {
    pub url: String,
    pub state_json: String,
    pub title: String,
}

pub struct HistoryStack {
    pub items: Vec<HistoryItem>,
    pub current_index: usize,
}

impl HistoryStack {
    pub fn new(initial_url: &str) -> Self {
        Self {
            items: vec![HistoryItem {
                url: initial_url.to_string(),
                state_json: "{}".to_string(),
                title: String::new(),
            }],
            current_index: 0,
        }
    }

    pub fn push_state(&mut self, url: &str, state_json: &str, title: &str) {
        self.items.truncate(self.current_index + 1);
        self.items.push(HistoryItem {
            url: url.to_string(),
            state_json: state_json.to_string(),
            title: title.to_string(),
        });
        self.current_index = self.items.len() - 1;
    }

    pub fn back(&mut self) -> Option<&HistoryItem> {
        if self.current_index > 0 {
            self.current_index -= 1;
            Some(&self.items[self.current_index])
        } else {
            None
        }
    }

    pub fn forward(&mut self) -> Option<&HistoryItem> {
        if self.current_index + 1 < self.items.len() {
            self.current_index += 1;
            Some(&self.items[self.current_index])
        } else {
            None
        }
    }

    pub fn export_history_nda(&self, session_id: &str) -> Vec<NdaTriple> {
        let mut triples = Vec::new();
        for (idx, item) in self.items.iter().enumerate() {
            let key = format!("history_{}", idx);
            triples.push(NdaTriple::new(
                session_id,
                190,
                &format!("{}:{}", key, item.url),
            ));
        }
        triples
    }
}
