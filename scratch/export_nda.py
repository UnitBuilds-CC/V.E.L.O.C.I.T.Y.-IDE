import os

def export_nda(file_path, output_txt_path):
    if not os.path.exists(file_path):
        print(f"{file_path} does not exist.")
        return
        
    with open(file_path, 'rb') as f:
        data = f.read()
        
    if len(data) < 9 or data[0:4] != b'NDAV':
        print("Invalid header.")
        return
        
    name_end = 8
    while name_end < len(data) and data[name_end] != 0:
        name_end += 1
        
    payload = data[name_end + 1:]
    
    with open(output_txt_path, 'w', encoding='utf-8') as out:
        out.write(payload.decode('utf-8', errors='ignore'))
    print(f"Exported {file_path} to {output_txt_path}")

if __name__ == "__main__":
    os.makedirs(r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\scratch", exist_ok=True)
    export_nda(
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\.velocity\sitemap.nda",
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\scratch\sitemap_unpacked.txt"
    )
    export_nda(
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\velocity-mcp\.velocity\changelog.nda",
        r"c:\Users\visse\OneDrive\Documentos\Kimi Code\velocity-workspace\scratch\changelog_unpacked.txt"
    )
