
const serverUrl = "";

async function api(url, body) {
    try {
        let options = body ? {method: "POST", body: JSON.stringify(body)} : {}
        const resp = await fetch(serverUrl + url, options);
        const text = await resp.text();
        return JSON.parse(text);
    } catch(e) {
        return {
            result: "ERR",
            message: e.message
        }
    }
}

export default api;
