import os
import sys

def unpack_nda(file_path):
    print(f"Reading NDA container: {file_path}")
    if not os.path.exists(file_path):
        print("File does not exist.")
        return
        
    with open(file_path, 'rb') as f:
        data = f.read()
        
    if len(data) < 9 or data[0:4] != b'NDAV':
        print("Invalid NDA container header.")
        return
        
    size = int.from_bytes(data[4:8], byteorder='little')
    
    name_end = 8
    while name_end < len(data) and data[name_end] != 0:
        name_end += 1
        
    filename = data[8:name_end].decode('utf-8', errors='ignore')
    payload = data[name_end + 1:]
    
    print(f"Metadata:")
    print(f"  Internal Filename: {filename}")
    print(f"  Payload Size: {size} bytes")
    print(f"  Actual Payload Slice Size: {len(payload)} bytes")
    print("--- Payload Preview (first 1000 chars) ---")
    try:
        text = payload.decode('utf-8')
        print(text[:1000])
        if len(text) > 1000:
            print("... [TRUNCATED] ...")
    except Exception as e:
        print(f"Binary payload (failed to decode as UTF-8): {e}")
        print(payload[:100])
    print("------------------------------------------\n")

if __name__ == "__main__":
    paths = [
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\.velocity\handover.nda",
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\.velocity\sitemap.nda",
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\.velocity\changelog.nda",
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\.velocity\transcript.nda"
    ]
    for path in paths:
        unpack_nda(path)
