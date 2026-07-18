import { NeoVis } from 'neovis.js';

const boltURL = "neo4j://localhost:7687";

async function initGraph() {
  const statusBadge = document.getElementById('status');
  
  try {
    const neovisConfig = {
      containerId: "viz",
      neo4j: {
        serverUrl: boltURL,
        serverUser: "neo4j",
        serverPassword: "agentic_secure_password"
      },
      labels: {
        Page: { 
          label: 'normalized_url', 
          font: { color: '#000000', face: 'Inter', size: 18, background: '#ffffff', strokeWidth: 0 },
          shape: 'box',
          color: { background: '#ffffff', border: '#000000' },
          margin: 15
        },
        Domain: { 
          label: 'name', 
          font: { color: '#000000', face: 'Inter', size: 16, background: '#ffffff', strokeWidth: 0 },
          shape: 'box',
          color: { background: '#ffffff', border: '#000000' },
          margin: 12
        },
        Endpoint: { 
          label: 'url', 
          font: { color: '#000000', face: 'Inter', size: 14, background: '#ffffff', strokeWidth: 0 },
          shape: 'box',
          color: { background: '#ffffff', border: '#4facfe' },
          margin: 10
        },
        Cookie: { 
          label: 'name', 
          font: { color: '#000000', face: 'Inter', size: 14, background: '#ffffff', strokeWidth: 0 },
          shape: 'box',
          color: { background: '#ffffff', border: '#000000' },
          margin: 10
        }
      },
      relationships: {
        LINKS_TO: { 
          label: 'LINKS_TO', 
          font: { color: '#000000', background: '#ffffff', size: 14, strokeWidth: 0, padding: 4 } 
        },
        USES_SCRIPT: { 
          label: ' ', // Remove redundant label
          font: { size: 0 }
        },
        USES_COOKIE: { 
          label: ' ', // Remove redundant label
          font: { size: 0 }
        }
      },
      visConfig: {
        nodes: {
          shape: 'box',
          font: { color: '#000000', face: 'Inter', strokeWidth: 0 },
          color: { 
            background: '#ffffff', 
            border: '#000000',
            highlight: { background: '#ffffff', border: '#4facfe' }
          },
          borderWidth: 2
        },
        edges: {
          font: { color: '#000000', background: '#ffffff', size: 12, strokeWidth: 0, padding: 4 },
          color: { color: '#4facfe', highlight: '#00d2ff' },
          width: 3,
          arrows: { to: { enabled: true, scaleFactor: 1.2 } },
          smooth: { enabled: true, type: 'dynamic' }
        },
        physics: {
          solver: 'forceAtlas2Based',
          forceAtlas2Based: {
            gravitationalConstant: -100,
            centralGravity: 0.005,
            springLength: 300,
            springConstant: 0.18,
            avoidOverlap: 1
          },
          stabilization: { enabled: true, iterations: 200 }
        }
      }
    };

    console.log("Initializing Neovis...");
    viz = new NeoVis(neovisConfig);
    
    console.log("Rendering graph...");
    viz.render("MATCH (n) OPTIONAL MATCH (n)-[r]->(m) RETURN n,r,m LIMIT 100");

    // Lockdown physics after 5s
    setTimeout(() => {
      if (viz && viz.network) {
        console.log("Locking physics...");
        viz.network.setOptions({ physics: { enabled: false } });
        viz.network.fit();
      }
    }, 5000);

    viz.registerOnEvent('clickNode', (event) => {
      if (event.node && event.node.raw) {
        showSummary(event.node.raw.properties);
      }
    });

    statusBadge.innerText = "Engine: Ghost Bridge v44.1 | Normalized";

  } catch (error) {
    console.error("Initialization Failed:", error);
    statusBadge.innerText = "Engine: Crash (Init Error)";
  }
}

let viz;

function showSummary(properties) {
  const sidebar = document.getElementById('sidebar');
  const summaryContent = document.getElementById('summary-content');
  const nodeTitle = document.getElementById('node-title');
  
  nodeTitle.innerText = properties.url || properties.name || "Infrastructure Node";
  
  let html = "";
  if (properties.title) html += `<p><strong>Title:</strong> ${properties.title}</p>`;
  if (properties.summary) html += `<p>${properties.summary}</p>`;
  
  if (properties.artifact) {
    const cleanPath = properties.artifact.replace(/\\/g, '/');
    html += `<div style="margin-top: 20px;">
      <button onclick="window.open('file:///${cleanPath}')" style="background: #4facfe; color: white; border: none; padding: 10px 20px; border-radius: 8px; cursor: pointer; width: 100%;">View Forensic Artifact</button>
    </div>`;
  }

  summaryContent.innerHTML = html;
  sidebar.classList.remove('hidden');
}

window.wipeDatabase = async () => {
  if (confirm("Wipe all gathered intelligence?")) {
    try {
      await fetch('http://localhost:8080/api/graph/wipe', { method: 'POST' });
      location.reload();
    } catch (e) {
      alert("API Offline");
    }
  }
};

document.getElementById('close-sidebar').addEventListener('click', () => {
  document.getElementById('sidebar').classList.add('hidden');
});

document.addEventListener('DOMContentLoaded', initGraph);
