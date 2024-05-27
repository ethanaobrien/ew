import { useState } from 'react'
import './Login.css'
import Request from '../Request.jsx'

function Login() {
    const error = useState(new URL(window.location).searchParams.get("message") || "");
    const uid = useState((window.localStorage && window.localStorage.getItem("ew_uid")) || "");
    let password;

    const handleSubmit = async (event) => {
        event.preventDefault();
        if (!uid[0] || !password || isNaN(uid[0])) return;
        if (window.localStorage) window.localStorage.setItem("ew_uid", uid[0]);
        let resp = await Request(
            "/api/webui/login",
            {
                uid: parseInt(uid[0]),
                password: password
            }
        );
        if (resp.result == "OK") {
            window.location.href = "/home/";
        } else {
            error[1](resp.message);
        }
    };
    
    const import_user = (e) => {
        e.preventDefault();
        window.location.href = "/import/";
    }

    const adminPanel = (e) => {
        e.preventDefault();
        window.location.href = "/admin/";
    }
  
    return (
        <div id="login-form">
            <h1>Login</h1>
            <form>
                <label htmlFor="id">SIF2 ID:</label>
                <input type="text" id="id" name="id" onChange={(event) => {uid[1](event.target.value)}} value={uid[0]} />
                <label htmlFor="password">Transfer passcode:</label>
                <input type="password" id="password" name="password" onChange={(event) => {password = event.target.value}} />
                <input type="submit" value="Submit" onClick={handleSubmit}/>
                <div id="sub_div">
                    <button onClick={import_user}>Import User</button><br/><br/>
                    <button hidden={!["127.0.0.1", "localhost"].includes(window.location.hostname)} onClick={adminPanel}>Admin panel</button>
                    { error[0] ? <p>Error: { error[0] } </p> : <p></p> }
                </div>
            </form>
        </div>
    );
}

export default Login;
