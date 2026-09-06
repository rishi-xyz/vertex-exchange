package server

import (
	"net/http"
	"net/url"
	"testing"
)

func req(t *testing.T, rawQuery string) *http.Request {
	t.Helper()
	return &http.Request{URL: &url.URL{RawQuery: rawQuery}}
}

func TestPaginationDefaults(t *testing.T) {
	limit, offset := pagination(req(t, ""))
	if limit != 50 || offset != 0 {
		t.Errorf("pagination() = %d, %d, want 50, 0", limit, offset)
	}
}

func TestPaginationClampsLimit(t *testing.T) {
	limit, _ := pagination(req(t, "limit=500"))
	if limit != 200 {
		t.Errorf("pagination(limit=500) limit = %d, want 200", limit)
	}
}

func TestPaginationIgnoresInvalidValues(t *testing.T) {
	limit, offset := pagination(req(t, "limit=-5&offset=-1"))
	if limit != 50 || offset != 0 {
		t.Errorf("pagination(limit=-5&offset=-1) = %d, %d, want defaults 50, 0", limit, offset)
	}
}

func TestPaginationCustomValues(t *testing.T) {
	limit, offset := pagination(req(t, "limit=10&offset=20"))
	if limit != 10 || offset != 20 {
		t.Errorf("pagination(limit=10&offset=20) = %d, %d, want 10, 20", limit, offset)
	}
}
