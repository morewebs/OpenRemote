// Package transport provides a full-duplex JSON-lines subprocess transport.
package transport

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"sync"
	"time"

	"github.com/morewebs/OpenRemote/internal/process"
)

type Message struct {
	JSONRPC string          `json:"jsonrpc,omitempty"`
	ID      json.RawMessage `json:"id,omitempty"`
	Method  string          `json:"method,omitempty"`
	Params  json.RawMessage `json:"params,omitempty"`
	Result  json.RawMessage `json:"result,omitempty"`
	Error   *Error          `json:"error,omitempty"`
}
type Error struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

func (e *Error) Error() string { return e.Message }

type Client struct {
	cmd       *exec.Cmd
	input     io.WriteCloser
	encoder   *json.Encoder
	writeMu   sync.Mutex
	mu        sync.Mutex
	pending   map[string]chan Message
	nextID    uint64
	done      chan struct{}
	closeOnce sync.Once
	onMessage func(Message)
	onExit    func(int)
}

func Start(ctx context.Context, bin string, args []string, cwd string, env map[string]string, onMessage func(Message), onStderr func([]byte), onExit func(int)) (*Client, error) {
	return StartDecoded(ctx, bin, args, cwd, env, onMessage, onStderr, onExit, func(data []byte) (Message, error) {
		var msg Message
		err := json.Unmarshal(data, &msg)
		return msg, err
	})
}

func StartDecoded(ctx context.Context, bin string, args []string, cwd string, env map[string]string, onMessage func(Message), onStderr func([]byte), onExit func(int), decode func([]byte) (Message, error)) (*Client, error) {
	cmd, err := process.CommandContext(ctx, bin, args...)
	if err != nil {
		return nil, err
	}
	cmd.Dir = cwd
	cmd.Env = os.Environ()
	for key, value := range env {
		cmd.Env = append(cmd.Env, key+"="+value)
	}
	input, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	output, err := cmd.StdoutPipe()
	if err != nil {
		_ = input.Close()
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		_ = input.Close()
		_ = output.Close()
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		_ = input.Close()
		_ = output.Close()
		_ = stderr.Close()
		return nil, err
	}
	c := &Client{cmd: cmd, input: input, encoder: json.NewEncoder(input), pending: make(map[string]chan Message), done: make(chan struct{}), onMessage: onMessage, onExit: onExit}
	stderrDone := make(chan struct{})
	go func() {
		defer close(stderrDone)
		buffer := make([]byte, 8192)
		for {
			n, err := stderr.Read(buffer)
			if n > 0 && onStderr != nil {
				onStderr(append([]byte(nil), buffer[:n]...))
			}
			if err != nil {
				return
			}
		}
	}()
	go func() {
		scanner := bufio.NewScanner(output)
		scanner.Buffer(make([]byte, 8192), 16*1024*1024)
		for scanner.Scan() {
			msg, err := decode(scanner.Bytes())
			if err != nil {
				continue
			}
			if msg.Method != "" {
				if c.onMessage != nil {
					c.onMessage(msg)
				}
			} else {
				c.mu.Lock()
				ch := c.pending[string(msg.ID)]
				c.mu.Unlock()
				if ch != nil {
					select {
					case ch <- msg:
					default:
					}
				}
			}
		}
		_ = input.Close()
		if scanner.Err() != nil {
			_ = cmd.Process.Kill()
		}
		<-stderrDone
		err := cmd.Wait()
		code := 0
		if err != nil {
			code = -1
			if cmd.ProcessState != nil {
				code = cmd.ProcessState.ExitCode()
			}
		}
		if c.onExit != nil {
			c.onExit(code)
		}
		close(c.done)
	}()
	return c, nil
}

func (c *Client) Send(value any) error {
	c.writeMu.Lock()
	defer c.writeMu.Unlock()
	select {
	case <-c.done:
		return fmt.Errorf("agent process stopped")
	default:
	}
	return c.encoder.Encode(value)
}

func (c *Client) Call(ctx context.Context, method string, params any, result any) error {
	return c.CallCommand(ctx, map[string]any{"jsonrpc": "2.0", "method": method, "params": params}, result)
}

// CallCommand correlates any JSON-lines request protocol using a string id.
// The caller owns command and must not reuse it concurrently.
func (c *Client) CallCommand(ctx context.Context, command map[string]any, result any) error {
	c.mu.Lock()
	c.nextID++
	id := fmt.Sprintf("openremote-%d", c.nextID)
	idJSON, _ := json.Marshal(id)
	ch := make(chan Message, 1)
	c.pending[string(idJSON)] = ch
	c.mu.Unlock()
	defer func() { c.mu.Lock(); delete(c.pending, string(idJSON)); c.mu.Unlock() }()
	command["id"] = id
	if err := c.Send(command); err != nil {
		return err
	}
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-c.done:
		return fmt.Errorf("agent process stopped")
	case msg := <-ch:
		if msg.Error != nil {
			return msg.Error
		}
		if result != nil {
			return json.Unmarshal(msg.Result, result)
		}
		return nil
	}
}

func (c *Client) Notify(method string, params any) error {
	return c.Send(map[string]any{"jsonrpc": "2.0", "method": method, "params": params})
}
func (c *Client) Reply(id json.RawMessage, result any) error {
	return c.Send(map[string]any{"jsonrpc": "2.0", "id": id, "result": result})
}
func (c *Client) Close() error {
	c.closeOnce.Do(func() {
		_ = c.input.Close()
		select {
		case <-c.done:
		case <-time.After(2 * time.Second):
			_ = c.cmd.Process.Kill()
			<-c.done
		}
	})
	return nil
}
