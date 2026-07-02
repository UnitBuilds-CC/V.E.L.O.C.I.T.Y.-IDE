FROM python:3.12-slim
RUN apt-get update && apt-get install -y git ripgrep build-essential curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt

ENV PYTHONPATH=/workspace
ENV VELOCITY_WORKSPACE=/workspace
CMD ["python", "agent/main.py"]
