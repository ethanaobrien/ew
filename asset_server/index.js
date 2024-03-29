const express = require('express')
const app = express()
const fs = require('fs');
var http = require('http');

const ip = '0.0.0.0';
const port = 8887;
//https://lovelive-schoolidolfestival2-album.akamaized.net
app.use(function (req, res, next) {
	console.log(req.method, ":", req.url);
	next();
})
function createDirFromFile(path) {
    fs.mkdirSync(require('path').dirname(path), { recursive: true });
}

app.get('/*', function (req, res) {
    const expectedPath = __dirname + "/resources"+req.url;
    createDirFromFile(expectedPath);
    let downloading = [];
    if (fs.existsSync(expectedPath)) {
        res.sendFile(expectedPath)
    } else {
        let url = (req.url.toLowerCase().startsWith("/android") || req.url.toLowerCase().startsWith("/ios") ? "https://lovelive-schoolidolfestival2-assets.akamaized.net" : "https://lovelive-schoolidolfestival2-album.akamaized.net") + req.url;
        const request = require('https').get(url, function(response) {
           response.pipe(res);
        });
        if (downloading.includes(req.url)) return;
        require('https').get(url, function(response) {
            console.log("Downloading " + req.url);
            downloading.push(req.url);
            const file = fs.createWriteStream(expectedPath);
            response.pipe(file);

            // after download completed close filestream
            file.on("finish", () => {
               file.close();
               console.log("Download Completed " + req.url);
            });
        })
    }
})

var httpsServer = http.createServer(app);


httpsServer.listen(port, ip, () => {
	let url = 'http://' + ip + ':' + port
	console.log('Server is listening at', url)
})
