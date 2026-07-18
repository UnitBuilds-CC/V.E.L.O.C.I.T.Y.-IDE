# Use ultra-lightweight Alpine Linux
FROM alpine:latest

WORKDIR /app

# Install Chromium, Xvfb, and xdotool
RUN apk add --no-cache \
    chromium \
    xvfb \
    xdotool \
    bash \
    ca-certificates

# Copy the pre-built binary
COPY agentic-mcp .

# Copy the extension folder (Required by the browser!)
COPY extension ./extension

# Create entrypoint script using printf
RUN printf '#!/bin/bash\nXvfb :99 -screen 0 1280x1024x24 &\nexport DISPLAY=:99\nsleep 2\n./agentic-mcp\n' > /app/entrypoint.sh && chmod +x /app/entrypoint.sh

# Set the entrypoint
ENTRYPOINT ["/app/entrypoint.sh"]
