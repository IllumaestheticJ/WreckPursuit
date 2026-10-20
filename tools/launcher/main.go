// WreckPursuit launcher: serves the "game" folder next to this program on a local port and
// opens the default browser. Everything stays on this computer.
package main

import (
	"fmt"
	"mime"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
)

func openBrowser(url string) {
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "windows":
		cmd = exec.Command("rundll32", "url.dll,FileProtocolHandler", url)
	case "darwin":
		cmd = exec.Command("open", url)
	default:
		cmd = exec.Command("xdg-open", url)
	}
	_ = cmd.Start()
}

func main() {
	exe, err := os.Executable()
	if err != nil {
		fmt.Println("cannot find myself:", err)
		return
	}
	dir := filepath.Join(filepath.Dir(exe), "game")
	if _, err := os.Stat(filepath.Join(dir, "index.html")); err != nil {
		fmt.Println("the 'game' folder must sit next to this program:", dir)
		fmt.Println("press Enter to close")
		fmt.Scanln()
		return
	}
	_ = mime.AddExtensionType(".wasm", "application/wasm")
	_ = mime.AddExtensionType(".ron", "text/plain; charset=utf-8")
	_ = mime.AddExtensionType(".ogg", "audio/ogg")
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		fmt.Println("cannot open a local port:", err)
		return
	}
	url := fmt.Sprintf("http://%s/", ln.Addr().String())
	fmt.Println("WreckPursuit is running at", url)
	fmt.Println("Your browser should open by itself; if not, paste that address into it.")
	fmt.Println("Close this window to stop the game.")
	openBrowser(url)
	fs := http.FileServer(http.Dir(dir))
	_ = http.Serve(ln, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-cache")
		fs.ServeHTTP(w, r)
	}))
}
