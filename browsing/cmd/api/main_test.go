package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	browserpkg "github.com/reclamation-admin/agentic-browser-go/pkg/browser"
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

func TestRuntimeSessionLifecycleEndpoints(t *testing.T) {
	originalOpen := openRuntimeSessionFn
	defer func() { openRuntimeSessionFn = originalOpen }()

	var openedReq runtimeOpenSessionRequest
	ctx, cancel := context.WithCancel(context.Background())
	stubSession := &browserpkg.Session{Ctx: ctx, Cancel: cancel}
	createdAt := time.Unix(1700000000, 0).UTC()
	openRuntimeSessionFn = func(req runtimeOpenSessionRequest) (*runtimeSessionEntry, []string, error) {
		openedReq = req
		return &runtimeSessionEntry{
			ID:         "rt-test",
			Mode:       "managed",
			CreatedAt:  createdAt,
			LastAction: "navigate",
			Session:    stubSession,
		}, []string{"navigated"}, nil
	}

	router := buildRouter(func() (runtimeBrowser, error) { return &stubRuntimeBrowser{}, nil })
	body, _ := json.Marshal(runtimeOpenSessionRequest{StartURL: "https://example.com", WaitTimeoutMs: 3210})
	openReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session", bytes.NewReader(body))
	openReq.Header.Set("Content-Type", "application/json")
	openRecorder := httptest.NewRecorder()

	router.ServeHTTP(openRecorder, openReq)

	if openRecorder.Code != http.StatusOK {
		t.Fatalf("expected 200 opening session, got %d with body %s", openRecorder.Code, openRecorder.Body.String())
	}
	if openedReq.StartURL != "https://example.com" || openedReq.WaitTimeoutMs != 3210 {
		t.Fatalf("unexpected open request captured: %+v", openedReq)
	}
	var openResp runtimeOpenSessionResponse
	if err := json.Unmarshal(openRecorder.Body.Bytes(), &openResp); err != nil {
		t.Fatalf("unmarshal open response: %v", err)
	}
	if openResp.SessionID != "rt-test" || !openResp.RuntimeState.Alive || openResp.RuntimeState.LastAction != "navigate" {
		t.Fatalf("unexpected open response: %+v", openResp)
	}
	if len(openResp.Warnings) != 1 || openResp.Warnings[0] != "navigated" {
		t.Fatalf("unexpected open warnings: %+v", openResp.Warnings)
	}

	closeReq := httptest.NewRequest(http.MethodDelete, "/api/runtime/session/rt-test", nil)
	closeRecorder := httptest.NewRecorder()

	router.ServeHTTP(closeRecorder, closeReq)

	if closeRecorder.Code != http.StatusOK {
		t.Fatalf("expected 200 closing session, got %d with body %s", closeRecorder.Code, closeRecorder.Body.String())
	}
	if stubSession.IsAlive() {
		t.Fatal("expected runtime session context to be canceled on close")
	}

	missingCloseReq := httptest.NewRequest(http.MethodDelete, "/api/runtime/session/rt-test", nil)
	missingCloseRecorder := httptest.NewRecorder()

	router.ServeHTTP(missingCloseRecorder, missingCloseReq)
	if missingCloseRecorder.Code != http.StatusNotFound {
		t.Fatalf("expected 404 closing missing session, got %d", missingCloseRecorder.Code)
	}
}

func TestRuntimeSessionCaptureEndpointUsesStoredSession(t *testing.T) {
	originalOpen := openRuntimeSessionFn
	originalCapture := captureRuntimeSessionFn
	defer func() {
		openRuntimeSessionFn = originalOpen
		captureRuntimeSessionFn = originalCapture
	}()

	stubSession := &browserpkg.Session{Ctx: context.Background()}
	entry := &runtimeSessionEntry{ID: "rt-capture", Mode: "managed", CreatedAt: time.Unix(1700000001, 0).UTC(), Session: stubSession}
	openRuntimeSessionFn = func(req runtimeOpenSessionRequest) (*runtimeSessionEntry, []string, error) {
		return entry, nil, nil
	}
	captureCalls := 0
	captureRuntimeSessionFn = func(got *runtimeSessionEntry) (*runtimeSessionCaptureResponse, error) {
		captureCalls++
		if got != entry {
			t.Fatalf("capture received wrong session entry: %+v", got)
		}
		return &runtimeSessionCaptureResponse{SessionID: got.ID, FinalURL: "https://example.com/final", RuntimeState: runtimeStateFromEntry(got)}, nil
	}

	router := buildRouter(func() (runtimeBrowser, error) { return &stubRuntimeBrowser{}, nil })
	openReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session", bytes.NewReader([]byte(`{}`)))
	openReq.Header.Set("Content-Type", "application/json")
	router.ServeHTTP(httptest.NewRecorder(), openReq)

	captureReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session/rt-capture/capture", bytes.NewReader([]byte(`{}`)))
	captureReq.Header.Set("Content-Type", "application/json")
	captureRecorder := httptest.NewRecorder()

	router.ServeHTTP(captureRecorder, captureReq)

	if captureRecorder.Code != http.StatusOK {
		t.Fatalf("expected 200 capture, got %d with body %s", captureRecorder.Code, captureRecorder.Body.String())
	}
	if captureCalls != 1 {
		t.Fatalf("expected 1 capture call, got %d", captureCalls)
	}

	missingReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session/missing/capture", bytes.NewReader([]byte(`{}`)))
	missingReq.Header.Set("Content-Type", "application/json")
	missingRecorder := httptest.NewRecorder()
	router.ServeHTTP(missingRecorder, missingReq)
	if missingRecorder.Code != http.StatusNotFound {
		t.Fatalf("expected 404 for missing capture session, got %d", missingRecorder.Code)
	}
}

func TestRuntimeSessionActionEndpointReturnsCaptureWithAction(t *testing.T) {
	originalOpen := openRuntimeSessionFn
	originalAction := performRuntimeActionFn
	originalCapture := captureRuntimeSessionFn
	defer func() {
		openRuntimeSessionFn = originalOpen
		performRuntimeActionFn = originalAction
		captureRuntimeSessionFn = originalCapture
	}()

	stubSession := &browserpkg.Session{Ctx: context.Background()}
	entry := &runtimeSessionEntry{ID: "rt-action", Mode: "managed", CreatedAt: time.Unix(1700000002, 0).UTC(), Session: stubSession}
	openRuntimeSessionFn = func(req runtimeOpenSessionRequest) (*runtimeSessionEntry, []string, error) {
		return entry, nil, nil
	}
	var actionReq runtimeSessionActionRequest
	performRuntimeActionFn = func(got *runtimeSessionEntry, req runtimeSessionActionRequest) (*runtimeActionResult, error) {
		if got != entry {
			t.Fatalf("action received wrong session entry: %+v", got)
		}
		actionReq = req
		return &runtimeActionResult{Action: "click", Target: "#submit", WaitAppliedMs: 900}, nil
	}
	captureRuntimeSessionFn = func(got *runtimeSessionEntry) (*runtimeSessionCaptureResponse, error) {
		return &runtimeSessionCaptureResponse{SessionID: got.ID, FinalURL: "https://example.com/after", RuntimeState: runtimeStateFromEntry(got)}, nil
	}

	router := buildRouter(func() (runtimeBrowser, error) { return &stubRuntimeBrowser{}, nil })
	openReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session", bytes.NewReader([]byte(`{}`)))
	openReq.Header.Set("Content-Type", "application/json")
	router.ServeHTTP(httptest.NewRecorder(), openReq)

	actionBody := []byte(`{"action":"click","selector":"#submit","waitTimeoutMs":900}`)
	actionReqHTTP := httptest.NewRequest(http.MethodPost, "/api/runtime/session/rt-action/action", bytes.NewReader(actionBody))
	actionReqHTTP.Header.Set("Content-Type", "application/json")
	actionRecorder := httptest.NewRecorder()

	router.ServeHTTP(actionRecorder, actionReqHTTP)

	if actionRecorder.Code != http.StatusOK {
		t.Fatalf("expected 200 action, got %d with body %s", actionRecorder.Code, actionRecorder.Body.String())
	}
	if actionReq.Action != "click" || actionReq.Selector != "#submit" || actionReq.WaitTimeoutMs != 900 {
		t.Fatalf("unexpected action request captured: %+v", actionReq)
	}
	var actionResp runtimeSessionCaptureResponse
	if err := json.Unmarshal(actionRecorder.Body.Bytes(), &actionResp); err != nil {
		t.Fatalf("unmarshal action response: %v", err)
	}
	if actionResp.Action == nil || actionResp.Action.Action != "click" || actionResp.Action.Target != "#submit" {
		t.Fatalf("unexpected action response payload: %+v", actionResp.Action)
	}

	badJSONReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session/rt-action/action", bytes.NewReader([]byte(`{"action":`)))
	badJSONReq.Header.Set("Content-Type", "application/json")
	badJSONRecorder := httptest.NewRecorder()
	router.ServeHTTP(badJSONRecorder, badJSONReq)
	if badJSONRecorder.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for malformed action JSON, got %d", badJSONRecorder.Code)
	}
}

func TestRuntimeSessionActionEndpointReturnsEvaluateResult(t *testing.T) {
	originalOpen := openRuntimeSessionFn
	originalAction := performRuntimeActionFn
	originalCapture := captureRuntimeSessionFn
	defer func() {
		openRuntimeSessionFn = originalOpen
		performRuntimeActionFn = originalAction
		captureRuntimeSessionFn = originalCapture
	}()

	stubSession := &browserpkg.Session{Ctx: context.Background()}
	entry := &runtimeSessionEntry{ID: "rt-eval", Mode: "managed", CreatedAt: time.Unix(1700000003, 0).UTC(), Session: stubSession}
	openRuntimeSessionFn = func(req runtimeOpenSessionRequest) (*runtimeSessionEntry, []string, error) {
		return entry, nil, nil
	}
	performRuntimeActionFn = func(got *runtimeSessionEntry, req runtimeSessionActionRequest) (*runtimeActionResult, error) {
		if got != entry {
			t.Fatalf("action received wrong session entry: %+v", got)
		}
		if req.Action != "evaluate" || req.Script != "({ answer: 42 })" {
			t.Fatalf("unexpected evaluate request captured: %+v", req)
		}
		return &runtimeActionResult{Action: "evaluate", Script: req.Script, Result: `{"answer":42}`, WaitAppliedMs: 600}, nil
	}
	captureRuntimeSessionFn = func(got *runtimeSessionEntry) (*runtimeSessionCaptureResponse, error) {
		return &runtimeSessionCaptureResponse{SessionID: got.ID, FinalURL: "https://example.com/eval", RuntimeState: runtimeStateFromEntry(got)}, nil
	}

	router := buildRouter(func() (runtimeBrowser, error) { return &stubRuntimeBrowser{}, nil })
	openReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session", bytes.NewReader([]byte(`{}`)))
	openReq.Header.Set("Content-Type", "application/json")
	router.ServeHTTP(httptest.NewRecorder(), openReq)

	evalBody := []byte(`{"action":"evaluate","script":"({ answer: 42 })","waitTimeoutMs":600}`)
	evalReqHTTP := httptest.NewRequest(http.MethodPost, "/api/runtime/session/rt-eval/action", bytes.NewReader(evalBody))
	evalReqHTTP.Header.Set("Content-Type", "application/json")
	evalRecorder := httptest.NewRecorder()

	router.ServeHTTP(evalRecorder, evalReqHTTP)

	if evalRecorder.Code != http.StatusOK {
		t.Fatalf("expected 200 evaluate action, got %d with body %s", evalRecorder.Code, evalRecorder.Body.String())
	}
	var evalResp runtimeSessionCaptureResponse
	if err := json.Unmarshal(evalRecorder.Body.Bytes(), &evalResp); err != nil {
		t.Fatalf("unmarshal evaluate response: %v", err)
	}
	if evalResp.Action == nil || evalResp.Action.Action != "evaluate" || evalResp.Action.Script != "({ answer: 42 })" || evalResp.Action.Result != `{"answer":42}` {
		t.Fatalf("unexpected evaluate action response payload: %+v", evalResp.Action)
	}
}
