import React from 'react'
import ReactDOM from 'react-dom/client'
import Login from './login/Login.jsx'
import Home from './home/Home.jsx'
//import './index.css'

let Elem;
switch (window.location.pathname) {
    case "/":
        Elem = Login;
        break;
    case "/home/":
        Elem = Home;
        break;
    default:
        window.location.pathname = "/";
}

ReactDOM.createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <Elem />
  </React.StrictMode>,
)
