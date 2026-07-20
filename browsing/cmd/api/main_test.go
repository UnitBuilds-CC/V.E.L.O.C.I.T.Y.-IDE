package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

type stubRuntimeBrowser struct {
	navigateURL     string
	capturedURL     string
	screenshotBytes []byte
	navigateErr     error
	waitErr         error
	screenshotErr   error
	currentURLErr   error
	closed          bool
}

func (s *stubRuntimeBrowser) Navigate(url string) error {
	s.navigateURL = url
	return s.navigateErr
}

func (s *stubRuntimeBrowser) WaitForStability(timeout time.Duration) error {
	return s.waitErr
}

func (s *stubRuntimeBrowser) CaptureScreenshot() ([]byte, error) {
	if s.screenshotErr != nil {
		return nil, s.screenshotErr
	}
	return s.screenshotBytes, nil
}

func (s *stubRuntimeBrowser) CurrentURL() (string, error) {
	if s.currentURLErr != nil {
		return "", s.currentURLErr
	}
	return s.capturedURL, nil
}

func (s *stubRuntimeBrowser) Close() {
	s.closed = true
}

func TestRuntimeVisualArtifactHandlerReturnsPng(t *testing.T) {
	stub := &stubRuntimeBrowser{
		capturedURL:     "https://example.com/final",
		screenshotBytes: []byte{0x89, 'P', 'N', 'G'},
	}
	router := buildRouter(func() (runtimeBrowser, error) { return stub, nil })
	body, _ := json.Marshal(RuntimeVisualArtifactRequest{URL: "https://example.com/start"})
	req := httptest.NewRequest(http.MethodPost, "/api/runtime/visual-artifact", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()

	router.ServeHTTP(recorder, req)

	if recorder.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d with body %s", recorder.Code, recorder.Body.String())
	}
	if got := recorder.Header().Get("Content-Type"); got != "image/png" {
		t.Fatalf("expected image/png content type, got %q", got)
	}
	if got := recorder.Header().Get("X-Runtime-Artifact-Kind"); got != "runtime_screenshot" {
		t.Fatalf("expected runtime artifact kind header, got %q", got)
	}
	if got := recorder.Header().Get("X-Runtime-Page-Url"); got != "https://example.com/final" {
		t.Fatalf("expected captured URL header, got %q", got)
	}
	if !bytes.Equal(recorder.Body.Bytes(), []byte{0x89, 'P', 'N', 'G'}) {
		t.Fatalf("unexpected PNG bytes: %v", recorder.Body.Bytes())
	}
	if stub.navigateURL != "https://example.com/start" {
		t.Fatalf("expected navigate URL to be recorded, got %q", stub.navigateURL)
	}
	if !stub.closed {
		t.Fatal("expected browser session to be closed")
	}
}

func TestRuntimeVisualArtifactHandlerRejectsBadUrl(t *testing.T) {
	router := buildRouter(func() (runtimeBrowser, error) {
		return &stubRuntimeBrowser{}, nil
	})
	body := []byte(`{"url":"not-a-url"}`)
	req := httptest.NewRequest(http.MethodPost, "/api/runtime/visual-artifact", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()

	router.ServeHTTP(recorder, req)

	if recorder.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d", recorder.Code)
	}
}

func TestRuntimeVisualArtifactHandlerReportsScreenshotFailure(t *testing.T) {
	stub := &stubRuntimeBrowser{
		screenshotErr: errors.New("capture failed"),
	}
	router := buildRouter(func() (runtimeBrowser, error) { return stub, nil })
	body, _ := json.Marshal(RuntimeVisualArtifactRequest{URL: "https://example.com/start"})
	req := httptest.NewRequest(http.MethodPost, "/api/runtime/visual-artifact", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	recorder := httptest.NewRecorder()

	router.ServeHTTP(recorder, req)

	if recorder.Code != http.StatusInternalServerError {
		t.Fatalf("expected 500, got %d", recorder.Code)
	}
	if !stub.closed {
		t.Fatal("expected browser session to be closed on failure")
	}
}
