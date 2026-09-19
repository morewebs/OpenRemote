package approval

import (
	"context"
	"errors"
	"testing"
)

func TestDeliveryFailureIsRetryable(t *testing.T) {
	r := NewRegistry(nil)
	defer r.Close()
	r.Put(&PendingApproval{ID: "a", SessionID: "s"})
	_, err := r.ResolveWith("a", true, "test", func(*PendingApproval) error { return errors.New("transport unavailable") })
	if err == nil {
		t.Fatal("delivery error hidden")
	}
	app, _ := r.Get("a")
	if app.Resolved {
		t.Fatal("failed delivery resolved approval")
	}
	app.Resolved = true
	stored, _ := r.Get("a")
	if stored.Resolved {
		t.Fatal("Get exposes mutable state")
	}
	if _, err := r.Resolve("a", true, "test"); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		approved, err := r.Wait(context.Background(), "a")
		if err != nil || !approved {
			t.Fatalf("wait = %v, %v", approved, err)
		}
	}
}
