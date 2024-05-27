import { useState, useParams, useEffect } from 'react'
import './Admin.css'
import Request from '../Request.jsx'

function Admin() {
    const [imp, setimp] = useState();
    const [exp, setexp] = useState();
    
    const handleSubmit = async (event) => {
        await Request(
            "/api/webui/admin",
            {
                import: !imp
            }
        );
        setimp(!imp);
    };
    const handleSubmit2 = async (event) => {
        await Request(
            "/api/webui/admin",
            {
                export: !exp
            }
        );
        setexp(!exp);
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
            <input type="checkbox" id="import" name="import" checked={!!imp} onClick={(i)=>handleSubmit(i)} />
            <label for="import">Allow account imports</label><br/><br/>
            <input type="checkbox" id="exp" name="exp" checked={!!exp} onClick={(i)=>handleSubmit2(i)} />
            <label for="exp">Allow account exports</label>
            </div>
        </div>
    );
}

export default Admin;
