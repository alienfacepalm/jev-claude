// Package jsjson is JSON with JavaScript semantics (SPEC 3.3, 3.6, 3.7).
//
// Values are: nil (null), Undefined, bool, float64, string (WTF-8, see jsstr), []any, and
// *Object, an object that keeps JavaScript's key order. Every body the proxy forwards and
// every file written goes through Stringify, never encoding/json.
package jsjson

import (
	"sort"
	"strconv"
)

type undefined struct{}

// Undefined is JavaScript's undefined. It can be stored as an object member, which Stringify
// then omits, exactly as `{...a, ...{x: undefined}}` behaves in JavaScript.
var Undefined any = undefined{}

// IsNullish reports `v == null` in JavaScript terms (null or undefined).
func IsNullish(v any) bool { return v == nil || v == Undefined }

// Object is a JavaScript object: canonical array-index keys first in ascending numeric order,
// then every other key in insertion order. Setting an existing key keeps its position.
type Object struct {
	keys []string
	vals map[string]any
}

// NewObject returns an empty object.
func NewObject() *Object { return &Object{vals: map[string]any{}} }

// Obj builds an object from alternating key, value arguments.
func Obj(kv ...any) *Object {
	o := NewObject()
	for i := 0; i+1 < len(kv); i += 2 {
		o.Set(kv[i].(string), kv[i+1])
	}
	return o
}

// IsIndexKey reports whether k is a canonical array index: a decimal integer with no leading
// zero (except "0" itself) below 2^32 - 1.
func IsIndexKey(k string) bool {
	if k == "" || len(k) > 10 {
		return false
	}
	if k[0] == '0' && len(k) > 1 {
		return false
	}
	for i := 0; i < len(k); i++ {
		if k[i] < '0' || k[i] > '9' {
			return false
		}
	}
	n, err := strconv.ParseUint(k, 10, 64)
	return err == nil && n < 4294967295
}

// Set sets k to v, appending k when new.
func (o *Object) Set(k string, v any) {
	if _, ok := o.vals[k]; !ok {
		o.keys = append(o.keys, k)
	}
	o.vals[k] = v
}

// Get returns the member and whether the key exists (a stored Undefined exists).
func (o *Object) Get(k string) (any, bool) {
	if o == nil {
		return Undefined, false
	}
	v, ok := o.vals[k]
	if !ok {
		return Undefined, false
	}
	return v, true
}

// Value returns the member, or Undefined.
func (o *Object) Value(k string) any {
	v, _ := o.Get(k)
	return v
}

// Has reports whether the key exists.
func (o *Object) Has(k string) bool {
	if o == nil {
		return false
	}
	_, ok := o.vals[k]
	return ok
}

// Delete removes k.
func (o *Object) Delete(k string) {
	if _, ok := o.vals[k]; !ok {
		return
	}
	delete(o.vals, k)
	for i, key := range o.keys {
		if key == k {
			o.keys = append(o.keys[:i:i], o.keys[i+1:]...)
			break
		}
	}
}

// Len is the number of keys.
func (o *Object) Len() int {
	if o == nil {
		return 0
	}
	return len(o.keys)
}

// Keys returns the keys in JavaScript order.
func (o *Object) Keys() []string {
	if o == nil {
		return nil
	}
	var index, other []string
	for _, k := range o.keys {
		if IsIndexKey(k) {
			index = append(index, k)
		} else {
			other = append(other, k)
		}
	}
	if len(index) == 0 {
		return other
	}
	sort.Slice(index, func(i, j int) bool {
		a, _ := strconv.ParseUint(index[i], 10, 64)
		b, _ := strconv.ParseUint(index[j], 10, 64)
		return a < b
	})
	return append(index, other...)
}

// Clone is a shallow copy, `{...o}`.
func (o *Object) Clone() *Object {
	c := NewObject()
	Spread(c, o)
	return c
}

// Spread copies src's own enumerable properties into dst, as `{...dst, ...src}` does: an
// object's members in key order, an array's elements under index keys, a string's UTF-16
// units under index keys; null, undefined, numbers and booleans add nothing.
func Spread(dst *Object, src any) {
	switch s := src.(type) {
	case *Object:
		for _, k := range s.Keys() {
			dst.Set(k, s.vals[k])
		}
	case []any:
		for i, v := range s {
			dst.Set(strconv.Itoa(i), v)
		}
	case string:
		for i, u := range utf16Units(s) {
			dst.Set(strconv.Itoa(i), u)
		}
	}
}

// Prop is `v?.[k]` for an object: the member, or Undefined for anything else.
func Prop(v any, k string) any {
	if o, ok := v.(*Object); ok {
		return o.Value(k)
	}
	return Undefined
}

// Path follows Prop through several keys, `v?.a?.b`.
func Path(v any, keys ...string) any {
	for _, k := range keys {
		v = Prop(v, k)
	}
	return v
}

// Coalesce is `a ?? b`.
func Coalesce(a, b any) any {
	if IsNullish(a) {
		return b
	}
	return a
}

// Truthy is JavaScript truthiness.
func Truthy(v any) bool {
	switch x := v.(type) {
	case nil, undefined:
		return false
	case bool:
		return x
	case float64:
		return x == x && x != 0
	case string:
		return x != ""
	}
	return true
}

// Str returns v when it is a string.
func Str(v any) (string, bool) {
	s, ok := v.(string)
	return s, ok
}

// AsObject returns v when it is an object.
func AsObject(v any) (*Object, bool) {
	o, ok := v.(*Object)
	return o, ok && o != nil
}
