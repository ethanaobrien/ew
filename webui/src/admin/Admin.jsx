import { useState, useParams, useEffect } from 'react'
import './Admin.css'
import Request from '../Request.jsx'

function Admin() {
    const [imp, setimp] = useState();
    const [exp, setexp] = useState();
    const ids = [setimp, setexp];
    
    const handleSubmit = async (id, event) => {
        await Request(
            "/api/webui/admin",
            {
                import: !!imp,
                export: !!exp
            }
        );
        ids[id](event.target.checked);
    };
    
    if (imp === undefined) {
        (async () => {
            let resp = await Request("/api/webui/admin");
            if (resp.result !== "OK") {
                window.location.href = "/?message=" + encodeURIComponent(resp.message);
                return;
            }
            setimp(resp.data.import);
            setexp(resp.data.export);
        })();
    }
  
    return (
        <div id="home">
            <h1>Admin</h1>
            <div>
            <input type="checkbox" id="import" name="import" checked={!!imp} onClick={(i)=>handleSubmit(0, i)} />
            <label for="import">Allow account imports</label><br/><br/>
            <input type="checkbox" id="exp" name="exp" checked={!!exp} onClick={(i)=>handleSubmit(1, i)} />
            <label for="exp">Allow account exports</label>
            </div>
        </div>
    );
}

export default Admin;
