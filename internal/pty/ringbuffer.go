package pty

import "sync"

// SlidingRingBuffer caps memory at maxBytes (default 4 MB) to prevent OOM
// during long streaming sessions. Spec goal.md: "Sliding Ring Buffers: 4-8MB"
type SlidingRingBuffer struct {
	mu       sync.Mutex
	buf      []byte
	cap      int
	writeHead int
	length   int
}

func NewSlidingRingBuffer(maxBytes int) *SlidingRingBuffer {
	if maxBytes <= 0 {
		maxBytes = 4 * 1024 * 1024
	}
	return &SlidingRingBuffer{buf: make([]byte, maxBytes), cap: maxBytes}
}

func (r *SlidingRingBuffer) Push(chunk []byte) {
	if len(chunk) == 0 {
		return
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if len(chunk) >= r.cap {
		tail := chunk[len(chunk)-r.cap:]
		copy(r.buf, tail)
		r.writeHead = 0
		r.length = r.cap
		return
	}
	avail := r.cap - r.writeHead
	if len(chunk) <= avail {
		copy(r.buf[r.writeHead:], chunk)
		r.writeHead = (r.writeHead + len(chunk)) % r.cap
	} else {
		copy(r.buf[r.writeHead:], chunk[:avail])
		copy(r.buf[0:], chunk[avail:])
		r.writeHead = len(chunk) - avail
	}
	if r.length+len(chunk) > r.cap {
		r.length = r.cap
	} else {
		r.length += len(chunk)
	}
}

func (r *SlidingRingBuffer) ReadAll() []byte {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.length == 0 {
		return nil
	}
	out := make([]byte, r.length)
	if r.length < r.cap {
		copy(out, r.buf[:r.length])
	} else {
		tail := r.cap - r.writeHead // actually bytes from writeHead to end
		// when full, oldest is at writeHead
		n := copy(out, r.buf[r.writeHead:])
		copy(out[n:], r.buf[:r.writeHead])
		_ = tail
	}
	return out
}

func (r *SlidingRingBuffer) Len() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.length
}

func (r *SlidingRingBuffer) Clear() {
	r.mu.Lock()
	r.writeHead = 0
	r.length = 0
	r.mu.Unlock()
}
