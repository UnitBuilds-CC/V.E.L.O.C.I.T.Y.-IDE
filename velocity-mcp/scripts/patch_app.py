import re

path = 'src/editor/app.rs'
with open(path, 'r', encoding='utf-8') as f:
    text = f.read()

old_save_active = '''    fn save_active(&mut self) {
        if let Some(id) = &self.active_tab {
            if let Some(path) = self.tab_path(id).cloned() {
                self.save_buffer_to(id, &path);
            } else {
                self.save_active_as();
            }
        } else {
            self.save_all();
        }
    }'''

new_save_active = '''    fn save_active(&mut self) {
        let active = self.active_tab.clone();
        if let Some(id) = active {
            if let Some(path) = self.tab_path(&id).cloned() {
                self.save_buffer_to(&id, &path);
            } else {
                self.save_active_as();
            }
        } else {
            self.save_all();
        }
    }'''

text = text.replace(old_save_active, new_save_active)

old_save_as = '''                    if ui.button("Save").clicked() {
                        if let Some(id) = &self.active_tab {
                            let p = self.workspace_root.join(&path_string);
                            self.save_buffer_to(id, &p);
                            // Update tab path so subsequent saves go to the same file.
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == *id) {
                                if let TabKind::Editor { ref mut path, .. } = tab.kind {
                                    *path = Some(p);
                                }
                            }
                            self.pending_save_as_path = None;
                        }
                    }'''

new_save_as = '''                    if ui.button("Save").clicked() {
                        if let Some(id) = self.active_tab.clone() {
                            let p = self.workspace_root.join(&path_string);
                            self.save_buffer_to(&id, &p);
                            // Update tab path so subsequent saves go to the same file.
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                                if let TabKind::Editor { ref mut path, .. } = tab.kind {
                                    *path = Some(p);
                                }
                            }
                            self.pending_save_as_path = None;
                        }
                    }'''

text = text.replace(old_save_as, new_save_as)

with open(path, 'w', encoding='utf-8') as f:
    f.write(text)

print("Patched")
