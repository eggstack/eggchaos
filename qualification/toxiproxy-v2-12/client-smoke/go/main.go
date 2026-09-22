// Command gosmoke exercises the eggchaos Toxiproxy compatibility surface
// with the pinned toxiproxy/v2 Go client (v2.12.0).
//
// Usage: go run . http://127.0.0.1:8474
package main

import (
	"encoding/json"
	"fmt"
	"os"

	toxiproxy "github.com/Shopify/toxiproxy/v2/client"
)

type step struct {
	Name   string `json:"name"`
	Status string `json:"status"`
	Detail string `json:"detail,omitempty"`
}

func main() {
	endpoint := "http://127.0.0.1:8474"
	if len(os.Args) > 1 {
		endpoint = os.Args[1]
	}
	var steps []step
	record := func(name string, err error, detail string) {
		status := "pass"
		if err != nil {
			status = fmt.Sprintf("FAIL: %v", err)
		}
		steps = append(steps, step{Name: name, Status: status, Detail: detail})
	}

	client := toxiproxy.NewClient(endpoint)
	version, err := client.Version()
	record("version", err, string(version))

	proxy, err := client.CreateProxy("smoke", "127.0.0.1:0", "127.0.0.1:1")
	if err == nil && proxy.Listen == "" {
		err = fmt.Errorf("empty listen in %#v", proxy)
	}
	listen := ""
	if err == nil {
		listen = proxy.Listen
	}
	record("create", err, listen)

	populated, err := client.Populate([]toxiproxy.Proxy{
		{Name: "smoke", Listen: listen, Upstream: "127.0.0.1:1", Enabled: true},
	})
	record("populate", err, fmt.Sprintf("%d proxies", len(populated)))

	var toxic *toxiproxy.Toxic
	if err == nil {
		toxic, err = proxy.AddToxic("lag", "latency", "downstream", 1.0, toxiproxy.Attributes{
			"latency": 50,
		})
	}
	detail := ""
	if toxic != nil {
		detail = toxic.Name
	}
	record("add-toxic", err, detail)

	var auto *toxiproxy.Toxic
	if err == nil {
		auto, err = proxy.AddToxic("", "bandwidth", "downstream", 1.0, toxiproxy.Attributes{
			"rate": 100,
		})
	}
	if auto != nil {
		detail = auto.Name
	}
	record("add-toxic-auto-name", err, detail)

	var updated *toxiproxy.Toxic
	if err == nil {
		updated, err = proxy.UpdateToxic("lag", 0.5, toxiproxy.Attributes{"latency": 100})
	}
	if updated != nil {
		detail = fmt.Sprintf("toxicity=%v", updated.Toxicity)
	}
	record("update-toxic", err, detail)

	toxics, terr := proxy.Toxics()
	if terr != nil {
		record("list-toxics", terr, "")
	} else {
		record("list-toxics", nil, fmt.Sprintf("%d toxics", len(toxics)))
	}

	record("remove-toxic", proxy.RemoveToxic("lag"), "")
	record("remove-toxic-auto", proxy.RemoveToxic("bandwidth_downstream"), "")
	record("disable", proxy.Disable(), "")
	record("enable", proxy.Enable(), "")
	record("reset", client.ResetState(), "")
	record("delete", proxy.Delete(), "")

	failed := 0
	for _, s := range steps {
		if len(s.Status) > 4 && s.Status[:4] == "FAIL" {
			failed++
		}
	}
	summary, _ := json.Marshal(map[string]interface{}{
		"client":  "github.com/Shopify/toxiproxy/v2@v2.12.0",
		"steps":   steps,
		"failed":  failed,
		"overall": map[string]bool{"pass": failed == 0},
	})
	fmt.Println(string(summary))
	if failed > 0 {
		os.Exit(1)
	}
}
