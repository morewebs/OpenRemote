package pty

import (
	"bytes"
	"sync"
	"testing"
)

func TestNewSlidingRingBuffer_DefaultSize(t *testing.T) {
	rb := NewSlidingRingBuffer(0)
	if rb.cap != 4*1024*1024 {
		t.Errorf("expected default cap 4MB, got %d", rb.cap)
	}
	rb2 := NewSlidingRingBuffer(-1)
	if rb2.cap != 4*1024*1024 {
		t.Errorf("expected default cap for negative input, got %d", rb2.cap)
	}
}

func TestNewSlidingRingBuffer_CustomSize(t *testing.T) {
	rb := NewSlidingRingBuffer(1024)
	if rb.cap != 1024 {
		t.Errorf("expected cap 1024, got %d", rb.cap)
	}
}

func TestPush_ReadAll_Empty(t *testing.T) {
	rb := NewSlidingRingBuffer(64)
	got := rb.ReadAll()
	if got != nil {
		t.Errorf("expected nil from empty buffer, got %v", got)
	}
	if rb.Len() != 0 {
		t.Errorf("expected Len 0, got %d", rb.Len())
	}
}

func TestPush_ReadAll_SimpleWrite(t *testing.T) {
	rb := NewSlidingRingBuffer(64)
	data := []byte("hello")
	rb.Push(data)

	got := rb.ReadAll()
	if !bytes.Equal(got, data) {
		t.Errorf("expected %q, got %q", data, got)
	}
	if rb.Len() != len(data) {
		t.Errorf("expected Len %d, got %d", len(data), rb.Len())
	}
}

func TestPush_EmptyChunk(t *testing.T) {
	rb := NewSlidingRingBuffer(64)
	rb.Push(nil)
	rb.Push([]byte{})
	if rb.Len() != 0 {
		t.Errorf("expected Len 0 after empty pushes, got %d", rb.Len())
	}
}

func TestPush_MultipleWrites(t *testing.T) {
	rb := NewSlidingRingBuffer(64)
	rb.Push([]byte("abc"))
	rb.Push([]byte("def"))
	rb.Push([]byte("ghi"))

	expected := []byte("abcdefghi")
	got := rb.ReadAll()
	if !bytes.Equal(got, expected) {
		t.Errorf("expected %q, got %q", expected, got)
	}
}

func TestPush_WrapAround(t *testing.T) {
	rb := NewSlidingRingBuffer(8)
	// Fill partially
	rb.Push([]byte("abcdef")) // 6 bytes, writeHead=6, length=6
	// This wraps: avail = 8-6 = 2, chunk len 4 > avail
	rb.Push([]byte("GHIJ")) // writes "GH" at [6..7], "IJ" at [0..1]

	// After the wrap, writeHead = 2 and the buffer is full. ReadAll drains
	// from writeHead to the end ("cdefGH"), then the wrapped head bytes
	// ("IJ"), so the buffer reads back in write order.
	expected := []byte("cdefGHIJ")
	got := rb.ReadAll()
	if !bytes.Equal(got, expected) {
		t.Errorf("expected %q, got %q", expected, got)
	}
	if rb.Len() != 8 {
		t.Errorf("expected Len 8, got %d", rb.Len())
	}
}

func TestPush_OverflowDiscardsOld(t *testing.T) {
	rb := NewSlidingRingBuffer(8)
	// Write more than capacity
	rb.Push([]byte("ABCDEFGHIJ")) // 10 bytes > cap 8
	// Should keep last 8 bytes: "CDEFGHIJ"
	expected := []byte("CDEFGHIJ")
	got := rb.ReadAll()
	if !bytes.Equal(got, expected) {
		t.Errorf("expected %q, got %q", expected, got)
	}
	if rb.Len() != 8 {
		t.Errorf("expected Len 8, got %d", rb.Len())
	}
}

func TestPush_ExactlyCapacity(t *testing.T) {
	rb := NewSlidingRingBuffer(8)
	rb.Push([]byte("12345678"))
	got := rb.ReadAll()
	expected := []byte("12345678")
	if !bytes.Equal(got, expected) {
		t.Errorf("expected %q, got %q", expected, got)
	}
}

func TestPush_OverflowThenReadAll(t *testing.T) {
	rb := NewSlidingRingBuffer(16)
	rb.Push([]byte("AAAAAAAA")) // 8 bytes
	rb.Push([]byte("BBBBBBBB")) // 8 bytes, now full at 16
	rb.Push([]byte("CCCC"))     // overflow: length stays 16, overwrites oldest

	got := rb.ReadAll()
	if len(got) != 16 {
		t.Fatalf("expected len 16, got %d", len(got))
	}
	// After third push: writeHead was 0 (after second push filled exactly).
	// Third push "CCCC": avail=16, chunk=4 <= avail → copy at [0..3], writeHead=4
	// length = min(16+4, 16) = 16
	// ReadAll full: read from writeHead(4) to end, then 0..4
	// buf[4:] = "AAAABBBB" (wait, second push wrote at offset 8)
	// Let me retrace:
	// After push1: buf="AAAAAAAA________", wh=8, len=8
	// After push2: avail=8, chunk=8 <= avail → copy at [8..15], wh=(8+8)%16=0, len=16
	//   buf="AAAAAAAABBBBBBBB"
	// After push3: avail=16-0=16, chunk=4 <= avail → copy at [0..3], wh=4, len=min(20,16)=16
	//   buf="CCCCAAAABBBBBBBB"
	// ReadAll full: n=copy(out, buf[4:])="AAAABBBBBBBB" (12 bytes), copy(out[12:], buf[:4])="CCCC"
	// out = "AAAABBBBBBBBCCCC"
	expected := []byte("AAAABBBBBBBBCCCC")
	if !bytes.Equal(got, expected) {
		t.Errorf("expected %q, got %q", expected, got)
	}
}

func TestClear(t *testing.T) {
	rb := NewSlidingRingBuffer(64)
	rb.Push([]byte("some data"))
	rb.Clear()

	if rb.Len() != 0 {
		t.Errorf("expected Len 0 after Clear, got %d", rb.Len())
	}
	got := rb.ReadAll()
	if got != nil {
		t.Errorf("expected nil after Clear, got %q", got)
	}

	// Verify we can write again after clear
	rb.Push([]byte("new data"))
	got = rb.ReadAll()
	if !bytes.Equal(got, []byte("new data")) {
		t.Errorf("expected %q after re-push, got %q", "new data", got)
	}
}

func TestConcurrency(t *testing.T) {
	rb := NewSlidingRingBuffer(1024)
	var wg sync.WaitGroup
	const goroutines = 50
	const iterations = 100

	for i := 0; i < goroutines; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < iterations; j++ {
				rb.Push([]byte("x"))
			}
		}()
	}

	for i := 0; i < goroutines/2; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < iterations; j++ {
				rb.ReadAll()
			}
		}()
	}

	for i := 0; i < goroutines/4; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < iterations; j++ {
				rb.Len()
			}
		}()
	}

	wg.Wait()

	// Just verify no panic/deadlock and length is within bounds
	length := rb.Len()
	if length > 1024 {
		t.Errorf("Len %d exceeds capacity 1024", length)
	}
}

func TestReadAll_DoesNotMutateBuffer(t *testing.T) {
	rb := NewSlidingRingBuffer(64)
	rb.Push([]byte("test"))

	first := rb.ReadAll()
	second := rb.ReadAll()
	if !bytes.Equal(first, second) {
		t.Error("consecutive ReadAll should return same content")
	}

	// Mutating the returned slice should not affect the buffer
	first[0] = 'X'
	third := rb.ReadAll()
	if !bytes.Equal(third, []byte("test")) {
		t.Error("mutating ReadAll result should not affect buffer")
	}
}
