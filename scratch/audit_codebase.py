import os
import glob
import re

def analyze_workspace():
    print("=== VELOCITY CODEBASE AUDIT ===")
    
    # 1. Monolith Files > 1000 LOC
    print("\n--- Monolith Files (>1000 LOC) ---")
    rust_files = glob.glob("c:/Users/visse/OneDrive/Documentos/Kimi Code/velocity-workspace/**/*.rs", recursive=True)
    go_files = glob.glob("e:/go-engine/**/*.go", recursive=True)
    
    all_files = rust_files + go_files
    monoliths = []
    
    for f in all_files:
        if "target" in f or "vendor" in f or ".git" in f:
            continue
        try:
            with open(f, 'r', encoding='utf-8', errors='ignore') as fp:
                lines = len(fp.readlines())
                if lines > 1000:
                    monoliths.append((lines, f))
        except Exception:
            pass
            
    monoliths.sort(reverse=True)
    for lines, path in monoliths:
        print(f"  {lines:5d} LOC: {path}")

    # 2. Check TODOs and FIXMEs
    print("\n--- TODOs and FIXMEs ---")
    todo_count = 0
    fixme_count = 0
    for f in all_files:
        if "target" in f or "vendor" in f or ".git" in f:
            continue
        try:
            with open(f, 'r', encoding='utf-8', errors='ignore') as fp:
                content = fp.read()
                todos = len(re.findall(r'\bTODO\b', content, re.IGNORECASE))
                fixmes = len(re.findall(r'\bFIXME\b', content, re.IGNORECASE))
                todo_count += todos
                fixme_count += fixmes
        except Exception:
            pass
    print(f"  Total TODOs: {todo_count}")
    print(f"  Total FIXMEs: {fixme_count}")

    # 3. Check unwrap / expect usage in production Rust code
    print("\n--- Error Handling Audit (unwraps/expects in production Rust) ---")
    unwrap_count = 0
    for f in rust_files:
        if "target" in f or "tests" in f or "_test.rs" in f:
            continue
        try:
            with open(f, 'r', encoding='utf-8', errors='ignore') as fp:
                content = fp.read()
                unwraps = len(re.findall(r'\.unwrap\(\)', content))
                unwrap_count += unwraps
        except Exception:
            pass
    print(f"  Total .unwrap() calls in production Rust files: {unwrap_count}")

if __name__ == "__main__":
    analyze_workspace()
