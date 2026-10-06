// Package router asks Jev which model fits a prompt (SPEC 6), ported from node/src/router.mjs.
package router

import (
	"context"
	"errors"
	"math"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jev"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/logx"
)

// Deadline is the hard wall-clock budget for one routing call.
var Deadline = config.JevDeadlineMs * time.Millisecond

// Args is one routing question.
type Args struct {
	Prompt        string
	Current       string
	ContextTokens float64
	Models        []config.Model
}

// Request builds the router request (SPEC 6.1).
func Request(a Args) *jsjson.Object {
	ids := make([]any, len(a.Models))
	for i, m := range a.Models {
		ids[i] = m.ID
	}
	questions := config.Questions()
	questions.Set("model", config.QuestionForModels(a.Models))
	return jsjson.Obj(
		"state", jsjson.Obj(
			"request", a.Prompt,
			"session", jsjson.Obj("current_model", a.Current, "context_tokens", a.ContextTokens),
			"environment", jsjson.Obj("available_models", ids),
		),
		"questions", questions,
	)
}

// Result builds the router result from a parsed Jev response, or an error where Node's
// destructuring would throw (SPEC 5.3, 6.2).
func Result(request *jsjson.Object, response any, contextTokens float64, ms float64) (*jsjson.Object, error) {
	if response == nil || response == jsjson.Undefined {
		return nil, errors.New("Cannot destructure the response")
	}
	answers := jsjson.Prop(response, "answers")
	if jsjson.IsNullish(answers) {
		return nil, errors.New("Cannot destructure property 'model' of 'result.answers' as it is undefined.")
	}
	score := func(name string) (float64, error) {
		q := jsjson.Prop(answers, name)
		if jsjson.IsNullish(q) {
			return 0, errors.New("Cannot read properties of undefined (reading 'score')")
		}
		return jsjson.ToNumber(jsjson.Prop(q, "score")) / config.ComplexityMaxScore, nil
	}
	task, err := score("task_complexity")
	if err != nil {
		return nil, err
	}
	reasoning, err := score("reasoning_required")
	if err != nil {
		return nil, err
	}
	tool, err := score("tool_complexity")
	if err != nil {
		return nil, err
	}
	out := jsjson.NewObject()
	jsjson.Spread(out, jsjson.Prop(answers, "model"))
	out.Set("request", request)
	out.Set("response", response)
	out.Set("metrics", jsjson.Obj(
		"taskComplexity", task,
		"reasoningRequired", reasoning,
		"toolComplexity", tool,
		"contextSize", math.Min(contextTokens/config.ContextWindowTokens, 1),
	))
	out.Set("ms", ms)
	return out, nil
}

// AskJev returns Jev's answer, or nil on any failure (logged), which policy reads as "keep
// the current model". No models means no request and no log line.
func AskJev(a Args) *jsjson.Object {
	if len(a.Models) == 0 {
		return nil
	}
	started := time.Now()
	ctx, cancel := context.WithTimeout(context.Background(), Deadline)
	defer cancel()
	request := Request(a)
	response, err := jev.SystemOne(ctx, request)
	var out *jsjson.Object
	if err == nil {
		out, err = Result(request, response, a.ContextTokens, float64(time.Since(started).Milliseconds()))
	}
	if err != nil {
		logx.Log("routing failed, keeping " + a.Current + ": " + err.Error())
		return nil
	}
	return out
}
