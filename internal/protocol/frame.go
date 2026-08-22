package protocol

import (
	"encoding/binary"
	"errors"
	"fmt"
)

// Opcode matches spec 04. Bit-for-bit so TS clients can drop-in.
type Opcode byte

const (
	OpcodePTYOutput     Opcode = 0x01 // Server -> Client: raw PTY bytes
	OpcodeKeystroke     Opcode = 0x02 // Client -> Server: raw keystroke / prompt
	OpcodeViewportResize Opcode = 0x03 // [cols:u16, rows:u16] big-endian
	OpcodeCatchup       Opcode = 0x04 // [lastSeq:u32] big-endian
	OpcodeJSONRPC       Opcode = 0x05 // UTF-8 JSON
	OpcodePingPong      Opcode = 0x06 // [ts:u64] big-endian
)

const headerSize = 2

type Frame struct {
	Opcode  Opcode
	Slot    byte
	Payload []byte
}

// Encode writes [opcode, slot, ...payload] — zero-copy of payload copy only once.
func Encode(opcode Opcode, slot byte, payload []byte) []byte {
	out := make([]byte, headerSize+len(payload))
	out[0] = byte(opcode)
	out[1] = slot
	copy(out[headerSize:], payload)
	return out
}

func EncodeString(opcode Opcode, slot byte, s string) []byte {
	return Encode(opcode, slot, []byte(s))
}

func Decode(data []byte) (Frame, error) {
	if len(data) < headerSize {
		return Frame{}, errors.New("frame: <2 bytes header")
	}
	return Frame{
		Opcode:  Opcode(data[0]),
		Slot:    data[1],
		Payload: data[headerSize:],
	}, nil
}

// Resize helpers (big-endian uint16 pair — spec 04).
func EncodeResize(cols, rows uint16) []byte {
	b := make([]byte, 4)
	binary.BigEndian.PutUint16(b[0:], cols)
	binary.BigEndian.PutUint16(b[2:], rows)
	return b
}

func DecodeResize(p []byte) (cols, rows uint16, err error) {
	if len(p) < 4 {
		return 0, 0, errors.New("resize payload: expected 4 bytes")
	}
	cols = binary.BigEndian.Uint16(p[0:])
	rows = binary.BigEndian.Uint16(p[2:])
	return cols, rows, nil
}

func EncodeCatchup(lastSeq uint32) []byte {
	b := make([]byte, 4)
	binary.BigEndian.PutUint32(b, lastSeq)
	return b
}

func DecodeCatchup(p []byte) (uint32, error) {
	if len(p) < 4 {
		return 0, errors.New("catchup payload: expected 4 bytes")
	}
	return binary.BigEndian.Uint32(p), nil
}

func EncodePing(ts uint64) []byte {
	b := make([]byte, 8)
	binary.BigEndian.PutUint64(b, ts)
	return b
}

func DecodePing(p []byte) (uint64, error) {
	if len(p) < 8 {
		return 0, errors.New("ping payload: expected 8 bytes")
	}
	return binary.BigEndian.Uint64(p), nil
}

func (o Opcode) String() string {
	switch o {
	case OpcodePTYOutput:
		return "PTY_OUTPUT(0x01)"
	case OpcodeKeystroke:
		return "KEYSTROKE(0x02)"
	case OpcodeViewportResize:
		return "RESIZE(0x03)"
	case OpcodeCatchup:
		return "CATCHUP(0x04)"
	case OpcodeJSONRPC:
		return "JSON_RPC(0x05)"
	case OpcodePingPong:
		return "PING_PONG(0x06)"
	default:
		return fmt.Sprintf("UNKNOWN(0x%02x)", byte(o))
	}
}
