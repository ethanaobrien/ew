import { useState } from 'react'
import './Import.css'
import Request from '../Request.jsx'

function Login() {
    const error = useState(new URL(window.location).searchParams.get("message") || "");
    const status = useState("");
    const uid = useState((window.localStorage && window.localStorage.getItem("ew_uid")) || "");
    let file=[], file1=[], file2=[], file3=[], password;
    let has_imported = false;

    const handleSubmit = async (event) => {
        event.preventDefault();
        if (!file[0] || !file1[0] || has_imported || !password) return;
        try {
            has_imported = true;
            let data = {
                userdata: JSON.parse(await file[0].text()),
                home: JSON.parse(await file1[0].text()),
                missions: file2[0] ? JSON.parse(await file2[0].text()) : undefined,
                sif_cards: file3[0] ? JSON.parse(await file3[0].text()) : undefined,
                password: password,
                jp: true
            };
            if (!data.userdata || !data.userdata.user || !data.userdata.user.id) {
                error[1]("Incorrect user data file format");
                return;
            }
            if (!data.home || !data.home.home || !data.home.home.information_list) {
                error[1]("Incorrect home data file format");
                return;
            }
            if (!Array.isArray(data.missions) && data.missions) {
                error[1]("Incorrect mission data file format");
                return;
            }
            if (!Array.isArray(data.sif_cards) && data.sif_cards) {
                error[1]("Incorrect sif card data file format");
                return;
            }
            let resp = await Request(
                "/api/webui/import",
                data
            );
            if (resp.result == "OK") {
                status[1](<div><p>Account imported!</p><p>User id: {resp.uid}</p><p>Migration token: {resp.migration_token}</p></div>);
            } else {
                error[1](resp.message);
            }
        } catch(e) {
            error[1](e.message);
        }
    };
  
    return (
        <div id="login-form">
            <h1>Transfer</h1>
            <form>
                { <p>{ status[0] } </p> }
                <label htmlFor="id">User data file (required):</label>
                <input type="file" id="id" name="id" onChange={(event) => {file = event.target.files}} accept="application/json"/>
                
                <label htmlFor="file1">User Home data file (required):</label>
                <input type="file" id="file1" name="file1" onChange={(event) => {file1 = event.target.files}} accept="application/json"/>
                
                <label htmlFor="file2">User Missions data file (optional):</label>
                <input type="file" id="file2" name="file2" onChange={(event) => {file2 = event.target.files}} accept="application/json"/>
                
                <label htmlFor="file3">Sif cards data file (optional):</label>
                <input type="file" id="file3" name="file3" onChange={(event) => {file3 = event.target.files}} accept="application/json"/>
                
                <label htmlFor="password">Transfer passcode (game will not recognize special characters, only use letters and numbers or you will be locked out):</label>
                <input type="password" id="password" name="password" onChange={(event) => {password = event.target.value}} />
                
                <input type="submit" value="Submit" onClick={handleSubmit}/>
                <div id="sub_div">
                    { error[0] ? <p>Error: { error[0] }. Please reload the page and try again.</p> : <p></p> }
                </div>
            </form>
        </div>
    );
}

export default Login;
