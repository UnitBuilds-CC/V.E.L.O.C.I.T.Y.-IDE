# Skill: Deep Interaction
Handling forms and sensitive data.

### 1. TYPE
- **Payload**: `{"action": "TYPE", "target": "backendId", "text": "content"}`
- **Usage**: Fill in non-sensitive text fields.

### 2. REQUEST_SECRET
- **Payload**: `{"action": "REQUEST_SECRET", "target": "backendId", "text": "key_name"}`
- **Usage**: Fill in passwords, pins, or sensitive usernames. This pulls from the secure vault.

### 3. WAIT
- **Payload**: `{"action": "WAIT", "text": "seconds"}`
- **Usage**: Wait for animations, dynamic content, or page transitions.
