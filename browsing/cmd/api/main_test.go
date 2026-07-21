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

func TestRuntimeSessionApplyStateEndpointForwardsRequest(t *testing.T) {
	originalOpen := openRuntimeSessionFn
	originalApply := applyRuntimeSessionStateFn
	defer func() {
		openRuntimeSessionFn = originalOpen
		applyRuntimeSessionStateFn = originalApply
	}()

	stubSession := &browserpkg.Session{Ctx: context.Background()}
	entry := &runtimeSessionEntry{ID: "rt-state", Mode: "managed", CreatedAt: time.Unix(1700000005, 0).UTC(), Session: stubSession}
	openRuntimeSessionFn = func(req runtimeOpenSessionRequest) (*runtimeSessionEntry, []string, error) {
		return entry, nil, nil
	}

	var appliedReq runtimeSessionApplyStateRequest
	applyRuntimeSessionStateFn = func(got *runtimeSessionEntry, req runtimeSessionApplyStateRequest) (*runtimeSessionApplyStateResponse, error) {
		if got != entry {
			t.Fatalf("apply state received wrong session entry: %+v", got)
		}
		appliedReq = req
		state := runtimeStateFromEntry(got)
		state.LastAction = "apply_state"
		return &runtimeSessionApplyStateResponse{
			SessionID:                  got.ID,
			AppliedCookieCount:         len(req.Cookies),
			AppliedCookieNames:         []string{"session", "csrf_token"},
			AppliedLocalStorageCount:   len(req.LocalStorage),
			AppliedLocalStorageKeys:    []string{"csrf_token"},
			AppliedSessionStorageCount: len(req.SessionStorage),
			AppliedSessionStorageKeys:  []string{"xsrf_nonce"},
			RuntimeState:               state,
			ProtocolEvidence:           protocolEvidenceFromEntry(got),
			Warnings:                   []string{"storage applied after navigation"},
		}, nil
	}

	router := buildRouter(func() (runtimeBrowser, error) { return &stubRuntimeBrowser{}, nil })
	openReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session", bytes.NewReader([]byte(`{}`)))
	openReq.Header.Set("Content-Type", "application/json")
	router.ServeHTTP(httptest.NewRecorder(), openReq)

	applyBody := []byte(`{"url":"https://example.com/login","cookies":[{"name":"session","value":"abc","domain":"example.com","path":"/login","secure":true,"httpOnly":true,"sameSite":"Lax","expiresUnix":1730000000,"sourceScheme":"Secure","sourcePort":443},{"name":"csrf_token","value":"seed","path":"/","session":true}],"localStorage":{"csrf_token":"local-seed"},"sessionStorage":{"xsrf_nonce":"session-seed"},"waitTimeoutMs":1200}`)
	applyReqHTTP := httptest.NewRequest(http.MethodPost, "/api/runtime/session/rt-state/state", bytes.NewReader(applyBody))
	applyReqHTTP.Header.Set("Content-Type", "application/json")
	applyRecorder := httptest.NewRecorder()

	router.ServeHTTP(applyRecorder, applyReqHTTP)

	if applyRecorder.Code != http.StatusOK {
		t.Fatalf("expected 200 apply state, got %d with body %s", applyRecorder.Code, applyRecorder.Body.String())
	}
	if appliedReq.URL != "https://example.com/login" || appliedReq.WaitTimeoutMs != 1200 {
		t.Fatalf("unexpected apply state request captured: %+v", appliedReq)
	}
	if len(appliedReq.Cookies) != 2 || appliedReq.Cookies[1].Name != "csrf_token" {
		t.Fatalf("unexpected applied cookies payload: %+v", appliedReq.Cookies)
	}
	if appliedReq.Cookies[0].Domain != "example.com" || appliedReq.Cookies[0].Path != "/login" || !appliedReq.Cookies[0].Secure || !appliedReq.Cookies[0].HTTPOnly {
		t.Fatalf("expected first cookie metadata to be preserved: %+v", appliedReq.Cookies[0])
	}
	if appliedReq.Cookies[0].SameSite != "Lax" || appliedReq.Cookies[0].ExpiresUnix == nil || *appliedReq.Cookies[0].ExpiresUnix != 1730000000 {
		t.Fatalf("expected same-site and expiry metadata to be preserved: %+v", appliedReq.Cookies[0])
	}
	if appliedReq.Cookies[0].SourceScheme != "Secure" || appliedReq.Cookies[0].SourcePort == nil || *appliedReq.Cookies[0].SourcePort != 443 {
		t.Fatalf("expected source metadata to be preserved: %+v", appliedReq.Cookies[0])
	}
	if got := appliedReq.LocalStorage["csrf_token"]; got != "local-seed" {
		t.Fatalf("unexpected local storage payload: %+v", appliedReq.LocalStorage)
	}
	if got := appliedReq.SessionStorage["xsrf_nonce"]; got != "session-seed" {
		t.Fatalf("unexpected session storage payload: %+v", appliedReq.SessionStorage)
	}
	var applyResp runtimeSessionApplyStateResponse
	if err := json.Unmarshal(applyRecorder.Body.Bytes(), &applyResp); err != nil {
		t.Fatalf("unmarshal apply state response: %v", err)
	}
	if applyResp.SessionID != "rt-state" || applyResp.AppliedCookieCount != 2 || applyResp.AppliedLocalStorageCount != 1 || applyResp.AppliedSessionStorageCount != 1 {
		t.Fatalf("unexpected apply state response payload: %+v", applyResp)
	}
	if len(applyResp.Warnings) != 1 || applyResp.Warnings[0] != "storage applied after navigation" {
		t.Fatalf("unexpected apply warnings: %+v", applyResp.Warnings)
	}
	if applyResp.RuntimeState.LastAction != "apply_state" {
		t.Fatalf("expected apply_state last action, got %+v", applyResp.RuntimeState)
	}

	missingReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session/missing/state", bytes.NewReader([]byte(`{}`)))
	missingReq.Header.Set("Content-Type", "application/json")
	missingRecorder := httptest.NewRecorder()
	router.ServeHTTP(missingRecorder, missingReq)
	if missingRecorder.Code != http.StatusNotFound {
		t.Fatalf("expected 404 for missing apply-state session, got %d", missingRecorder.Code)
	}

	badJSONReq := httptest.NewRequest(http.MethodPost, "/api/runtime/session/rt-state/state", bytes.NewReader([]byte(`{"cookies":`)))
	badJSONReq.Header.Set("Content-Type", "application/json")
	badJSONRecorder := httptest.NewRecorder()
	router.ServeHTTP(badJSONRecorder, badJSONReq)
	if badJSONRecorder.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for malformed apply-state JSON, got %d", badJSONRecorder.Code)
	}
}

func TestRuntimeSessionCaptureEndpointReturnsFrameShadowAndCanvasInventory(t *testing.T) {
	originalOpen := openRuntimeSessionFn
	originalCapture := captureRuntimeSessionFn
	defer func() {
		openRuntimeSessionFn = originalOpen
		captureRuntimeSessionFn = originalCapture
	}()

	stubSession := &browserpkg.Session{Ctx: context.Background()}
	entry := &runtimeSessionEntry{ID: "rt-capture", Mode: "managed", CreatedAt: time.Unix(1700000004, 0).UTC(), Session: stubSession}
	openRuntimeSessionFn = func(req runtimeOpenSessionRequest) (*runtimeSessionEntry, []string, error) {
		return entry, nil, nil
	}
	captureRuntimeSessionFn = func(got *runtimeSessionEntry) (*runtimeSessionCaptureResponse, error) {
		state := runtimeStateFromEntry(got)
		state.FrameCount = 2
		state.ShadowHostCount = 1
		state.CanvasCount = 2
		state.WebGLCanvasCount = 1
		return &runtimeSessionCaptureResponse{
			SessionID:    got.ID,
			FinalURL:     "https://example.com/capture",
			RuntimeState: state,
			Frames: []runtimeFrameSummary{
				{Selector: "iframe#checkout", Source: "https://payments.example/frame", Accessible: false, SameOrigin: false},
				{Selector: "iframe[name=embedded]", Source: "/embedded", Accessible: true, SameOrigin: true, SemanticNodeCount: 4},
			},
			ShadowHosts: []runtimeShadowHostSummary{
				{Selector: "checkout-shell", Tag: "checkout-shell", Mode: "open", SemanticNodeCount: 3, TextSample: "Pay now"},
			},
			Canvases: []runtimeCanvasSummary{
				{Selector: "canvas#stage", Width: 640, Height: 480, ContextKinds: []string{"2d"}, TextOpCount: 2, RuntimeEvidence: true, TextSample: "Sign in"},
				{Selector: "canvas#webgl", Width: 1024, Height: 768, ContextKinds: []string{"webgl"}, WebGLDrawCount: 4, LikelyAnimated: true, RuntimeEvidence: true},
			},
		}, nil
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
	var captureResp runtimeSessionCaptureResponse
	if err := json.Unmarshal(captureRecorder.Body.Bytes(), &captureResp); err != nil {
		t.Fatalf("unmarshal capture response: %v", err)
	}
	if captureResp.RuntimeState.FrameCount != 2 || captureResp.RuntimeState.ShadowHostCount != 1 {
		t.Fatalf("unexpected frame/shadow inventory counts: %+v", captureResp.RuntimeState)
	}
	if captureResp.RuntimeState.CanvasCount != 2 || captureResp.RuntimeState.WebGLCanvasCount != 1 {
		t.Fatalf("unexpected canvas inventory counts: %+v", captureResp.RuntimeState)
	}
	if len(captureResp.Frames) != 2 || captureResp.Frames[1].SemanticNodeCount != 4 {
		t.Fatalf("unexpected frame inventory payload: %+v", captureResp.Frames)
	}
	if len(captureResp.ShadowHosts) != 1 || captureResp.ShadowHosts[0].Mode != "open" {
		t.Fatalf("unexpected shadow host payload: %+v", captureResp.ShadowHosts)
	}
	if len(captureResp.Canvases) != 2 || captureResp.Canvases[1].WebGLDrawCount != 4 || !captureResp.Canvases[1].LikelyAnimated {
		t.Fatalf("unexpected canvas payload: %+v", captureResp.Canvases)
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
