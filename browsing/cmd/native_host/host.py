import sys
import struct
import json
import socket
import logging

logging.basicConfig(filename='E:\\LLM-Browser\\native_host_py.log', level=logging.DEBUG)
logging.info("Python Native Host Started")

def send_message(message):
    content = json.dumps(message).encode('utf-8')
    sys.stdout.buffer.write(struct.pack('I', len(content)))
    sys.stdout.buffer.write(content)
    sys.stdout.buffer.flush()

def read_message():
    text_length_bytes = sys.stdin.buffer.read(4)
    if len(text_length_bytes) == 0:
        return None
    text_length = struct.unpack('I', text_length_bytes)[0]
    chunks = []
    bytes_read = 0
    while bytes_read < text_length:
        chunk = sys.stdin.buffer.read(min(text_length - bytes_read, 4096))
        if not chunk:
            break
        chunks.append(chunk)
        bytes_read += len(chunk)
    return json.loads(b''.join(chunks).decode('utf-8'))

try:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect(('127.0.0.1', 9998))
    logging.info("Connected to Go app")
    
    # Simple relay loop would go here, but let's just log for now
    while True:
        msg = read_message()
        if msg is None: break
        logging.info(f"Received from Chrome: {msg}")
        s.sendall((json.dumps(msg) + "\n").encode('utf-8'))
except Exception as e:
    logging.error(f"Error: {e}")
