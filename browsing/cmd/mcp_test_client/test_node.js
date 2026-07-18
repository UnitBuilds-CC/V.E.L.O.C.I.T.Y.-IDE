const { Client } = require("@modelcontextprotocol/sdk/client/index.js");
const { StdioClientTransport } = require("@modelcontextprotocol/sdk/client/stdio.js");

async function run() {
    console.log("Starting Node MCP client...");
    const transport = new StdioClientTransport({
        command: "E:/LLM-Browser/agentic-browser-mcp.exe",
        args: []
    });

    const client = new Client(
        { name: "test-client", version: "1.0.0" },
        { capabilities: {} }
    );

    try {
        await client.connect(transport);
        console.log("Connected and initialized!");

        const tools = await client.listTools();
        console.log("Tools:", JSON.stringify(tools, null, 2));

    } catch (err) {
        console.error("MCP Error:", err);
    } finally {
        await client.close();
        process.exit(0);
    }
}

run();
