import urllib.request
urls = [
    "https://raw.githubusercontent.com/Adanos020/egui_dock/main/examples/simple.rs",
]
for url in urls:
    try:
        data = urllib.request.urlopen(url, timeout=15).read().decode()
        print(data[:4000])
    except Exception as e:
        print("error", e)
