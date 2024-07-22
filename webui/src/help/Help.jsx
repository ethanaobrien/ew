import { useState, useParams, useEffect } from 'react'
import './Help.css'
import Request from '../Request.jsx'
let init = false;

function Help() {
    const [downloadUrl, setDownloadUrl] = useState(<div>Contact your server admin, asking for a patched apk. Examples can be found <a href="https://ethanthesleepy.one/public/lovelive/sif2/">here</a>.</div>);
    const [downloadUrliOSGL, setDownloadUrliOSGL] = useState("https://ethanthesleepy.one/public/lovelive/sif2/sif2-gl.ipa");
    const [downloadUrliOSJP, setDownloadUrliOSJP] = useState("https://ethanthesleepy.one/public/lovelive/sif2/sif2-jp.ipa");

    const [assetUrl, setAssetUrl] = useState("https://sif2.sif.moe");
    
    useEffect(() => {
        (async () => {
            if (init) return;
            init = true;
            let resp = await Request("/api/webui/serverInfo");
            if (resp.result !== "OK") {
                return;
            }
            if (!resp.data.links) return;
            if (resp.data.links.global && resp.data.links.japan) {
                setDownloadUrl(
                    <div>Your server admin has a link to download! Download <a href={resp.data.links.japan}>Japan</a> or <a href={resp.data.links.global}>Global</a></div>
                );
            } else if (resp.data.links.global) {
                setDownloadUrl(
                    <div>Your server admin has a link to download! Download <a href={resp.data.links.global}>Global</a></div>
                );
            } else if (resp.data.links.japan) {
                setDownloadUrl(
                    <div>Your server admin has a link to download! Download <a href={resp.data.links.japan}>Japan</a></div>
                );
            }
            if (resp.data.links.assets) {
                setAssetUrl(resp.data.links.assets);
            }

            if (!resp.data.links.ios) return;
            if (resp.data.links.ios.japan) {
                setDownloadUrliOSJP(resp.data.links.ios.japan);
            }
            if (resp.data.links.ios.global) {
                setDownloadUrliOSGL(resp.data.links.ios.global);
            }

        })();
    });
  
    return (
        <div id="home">
            <h1>Help/About</h1>
            <h2>What is "ew"? What is this server for?</h2>
            <p>"ew" is a private server, written in Rust, for the short lived game "Love Live! School idol festival 2 MIRACLE LIVE!", a Love Live! themed mobile rhythm game.</p>

            <h2>So I got the server running, how do I install the app? (Android)</h2>
            <p>{downloadUrl}</p>

            <h2>So I got the server running, how do I install the app? (iOS)</h2>
            <p>Running on iOS is much simpler than Android, thanks to triangle on the discord. You first download an ipa file for <a href={downloadUrliOSGL}>global</a> or <a href={downloadUrliOSJP}>Japan</a>, and use <a href="https://sideloadly.io/">Sideloadly</a> (or your preferred application installer) to install the app. Then open settings, navigate to the app you just installed, and input the server url (Which is likely "{window.location.origin}", though this may not be the case), and the asset url, which is "{assetUrl}". If you have any errors opening the app, make sure none of the urls in settings end with a slash (/).</p>

            <h2>Help! I'm trying to open the app and it shows as "unavailable" (iOS)</h2>
            <p>Do not delete it, Just re-sideload the app. This is an Apple "security" feature.</p>

            <h2>How well does this work?</h2>
            <p>Works well enough. The server itself takes up not even 20mb of storage, and it's written in rust. I personally think it's pretty well written.</p>

            <h2>Could my computer/laptop run a server?</h2>
            <p>Very very likely. If the platform is <a href="https://doc.rust-lang.org/nightly/rustc/platform-support.html">supported by rust</a>, then the answer is yes! It is recommended to manually compile the project until I get the time to setup GitHub actions. <a href="https://github.com/ethanaobrien/ew">ew github repo</a></p>

            <h2>Is the server down right now? I can't connect</h2>
            <p>Assuming you have just loaded this page on the server you use, then the answer is no, otherwise please contact your server admin.</p>

            <h2>Do events work?</h2>
            <p>Most events do not, though most should not crash the game. Star events are partially implemented. You can get your rank up, and compete with other players in a ranking table, but no rewards are currently implemented.</p>

            <h2>But then, how do I get event URs?</h2>
            <p>There are serial codes for several things, one of which includes all the event URs. I don't remember what does what but it is recommended to look at the serial code file to get the latest codes.</p>

            <h2>Why does the game crash when I do x?</h2>
            <p>This likely means something on the server is broken. If you're self hosting, please open a GitHub issue. Otherwise, contact your server admin and ask them to report the issue.</p>
        </div>
    );
}

export default Help;
